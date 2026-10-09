//! Shared gateway for CLI and host adapters; never evaluates shell request text.
use mustard_core::domain::code_search::Request;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use serde_json::json;
use std::path::Path;

pub(crate) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub(crate) fn answer(
    cwd: &Path,
    request: &Request,
) -> Result<mustard_core::io::code_search::Answer, String> {
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
    let selector = if request.choose && !request.intent.trim().is_empty() {
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
                mustard_core::io::code_search::enrich(&tree,&tree,&cwd,request,&mut answer,selector.as_ref().map(|s|s as &dyn mustard_core::domain::knowledge::selection::SymbolSelector));
                answer.report["task_context"]["learning"]["scan"]=json!({"status":"refreshed-native","files_read":report.read.len(),"acknowledged":saved.is_ok(),"local_model_calls":0,"remote_model_calls":0});
            },
            Err(error)=>answer.report["task_context"]["learning"]["scan"]=json!({"status":"pending","reason":error.to_string()}),
        }
    }
    Ok(answer)
}

fn guard(root: &Path, cwd: &Path, request: &Request) -> Result<(), String> {
    let ctx = Ctx {
        project_dir: root.to_string_lossy().into_owned(),
        trigger: Some(Trigger::PreToolUse),
        config: mustard_core::ProjectConfig::load(root),
        ..Ctx::default()
    };
    let input = if request.tool == "Read" {
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
            ..HookInput::default()
        }
    };
    let verdict = if request.tool == "Read" {
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

/// Rewrite only one unwrapped, literal simple search. Preserve arguments and
/// cwd exactly; ambiguous shell syntax follows the original permission path.
pub(crate) fn before_bash(input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    if !ctx.config.search_answer() {
        return None;
    }
    let command = input.tool_input.get("command")?.as_str()?;
    if !literal_shell(command) {
        return None;
    }
    let segments = crate::hooks::bash::lex::segments(command);
    let segment = segments.first().filter(|_| segments.len() == 1)?;
    if !matches!(segment.program.raw.as_str(), "rg" | "grep" | "git")
        || !segment.leading.is_empty()
        || !segment.redirects.is_empty()
        || segment.piped
    {
        return None;
    }
    // The lexer strips wrappers and assignments. Refuse those before using its argv.
    if !command
        .trim_start()
        .strip_prefix(&segment.program.raw)
        .is_some_and(|rest| rest.starts_with(char::is_whitespace))
    {
        return None;
    }
    if segment
        .args
        .iter()
        .any(|arg| arg.raw == arg.text && arg.raw.contains(['*', '?', '[', '{', '~']))
    {
        return None;
    }
    let argv: Vec<_> = std::iter::once(segment.program.text.clone())
        .chain(segment.args.iter().map(|arg| arg.text.clone()))
        .collect();
    let mut request = Request::native(&argv).ok()?;
    mustard_core::io::code_search::arguments(&request).ok()?;
    request.intent = input
        .tool_description()
        .unwrap_or_default()
        .chars()
        .take(1000)
        .collect();
    let cwd = input.cwd.as_deref().unwrap_or(&ctx.project_dir);
    let replacement = format!(
        "mustard-rt run search --root {} --request {} --shell-output",
        quote(cwd),
        quote(&serde_json::to_string(&request).ok()?)
    );
    let mut tool_input = input.tool_input.clone();
    tool_input["command"] = json!(replacement);
    Some(Verdict::Rewrite {
        tool_input,
        note: None,
    })
}

fn literal_shell(command: &str) -> bool {
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

/// A classic hook cannot change a tool's name. Hand off a supported project
/// request to the registered gateway or this exact CLI request; never execute
/// the search in a permission hook and then execute it again in the tool.
pub(crate) fn before_file(input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    if !ctx.config.search_answer() {
        return None;
    }
    let tool = input.tool_name.as_deref()?;
    if !matches!(tool, "Grep" | "Glob" | "Read") {
        return None;
    }
    if tool != "Read" {
        use mustard_core::platform::code_tools::{MachineRunner, ToolRunner};
        let path = std::env::var("PATH").unwrap_or_default();
        if !MachineRunner::new(&path).on_path("rg") {
            return None;
        }
    }
    if tool == "Read"
        && let Some(target) = super::paths::WriteTarget::classify(&ctx.project_dir, input)
        && !matches!(
            target.class,
            super::paths::PathClass::Production | super::paths::PathClass::OutsideRepo
        )
    {
        return None;
    }
    let cwd = input.cwd.as_deref().unwrap_or(&ctx.project_dir);
    let given = input
        .tool_input
        .get(if tool == "Read" { "file_path" } else { "path" })
        .and_then(serde_json::Value::as_str)
        .unwrap_or(".");
    let path = super::code_route::project_path(&ctx.project_dir, cwd, given)?;
    let canonical = path.abs.canonicalize().ok()?;
    if !canonical.starts_with(path.tree.canonicalize().ok()?) {
        return None;
    }
    if tool == "Read" && (!path.abs.is_file() || path.abs.metadata().ok()?.len() > 2 * 1024 * 1024)
    {
        return None;
    }
    let request = Request {
        tool: tool.into(),
        input: input.tool_input.clone(),
        intent: input
            .tool_description()
            .unwrap_or_default()
            .chars()
            .take(1000)
            .collect(),
        purpose: Default::default(),
        choose: false,
    };
    if tool == "Read" {
        if input
            .tool_input
            .as_object()?
            .keys()
            .any(|key| !matches!(key.as_str(), "file_path" | "offset" | "limit"))
        {
            return None;
        }
        let bytes = std::fs::read(&path.abs).ok()?;
        if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
            return None;
        }
    } else {
        mustard_core::io::code_search::arguments(&request).ok()?;
    }
    let command = format!(
        "mustard-rt run search --root {} --request {} --shell-output",
        quote(cwd),
        quote(&serde_json::to_string(&request).ok()?)
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
            let parts = crate::hooks::bash::lex::segments(tool_input["command"].as_str().unwrap());
            let args = &parts[0].args;
            let at = args.iter().position(|arg| arg.text == "--request").unwrap();
            let request: Request = serde_json::from_str(&args[at + 1].text).unwrap();
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
    fn unknown_shell_effects_keep_the_original_execution_path() {
        let dir = tempfile::tempdir().unwrap();
        for command in [
            "rg x src | head",
            "cd src && rg x .",
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
    }
}
