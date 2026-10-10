use mustard_core::domain::code_search::Request;
use serde_json::json;
use std::io::Write;
use std::path::Path;

pub fn run(
    root: &Path,
    request: Option<&str>,
    argv: &[String],
    intent: Option<&str>,
    purpose: Option<&str>,
    choose: bool,
    raw: bool,
    shell_output: bool,
) {
    let delivery_context=request.and_then(|text|mustard_core::domain::code_search::contract::DeliveryContext::from_json(text).ok().flatten());
    let result = (|| {
        let mut request: Request = match request {
            Some(text) if text.len() <= 64 * 1024 => Request::from_json(text)?,
            Some(_) => return Err("search-request-too-large".into()),
            None => Request::native(argv)?,
        };
        if let Some(intent) = intent {
            request.intent = intent.into();
        }
        if let Some(purpose) = purpose {
            request.purpose =
                mustard_core::domain::knowledge::investigation::Purpose::parse(purpose)
                    .ok_or("search-invalid-purpose")?;
        }
        request.choose |= choose;
        if raw && !matches!(request.tool.as_str(), "rg" | "grep" | "git") {
            return Err("search-raw-output-requires-native-tool".into());
        }
        crate::shared::search_gateway::answer(root, &request).map(|answer| (answer, request))
    })();
    match result {
        Ok((mut answer, request)) => {
            if raw || shell_output {
                let git=mustard_core::platform::git::run(root,&["rev-parse","--show-toplevel"]);
                let tree=if git.ok {std::path::PathBuf::from(git.stdout.trim())}else{root.to_path_buf()};
                let receipt=if shell_output {delivery_context.as_ref().and_then(|context|
                    mustard_core::io::code_search::delivery::prepare(&mut answer,&request,&tree,context))}else{None};
                let mut output = if shell_output {
                    mustard_core::io::code_search::presentation::agent(&answer, &request, root)
                        .stdout
                } else {
                    answer.stdout
                };
                if let Some(token)=receipt {
                    let receipt=mustard_core::io::code_search::delivery::receipt(&token,&output);
                    output.extend_from_slice(receipt.as_bytes());
                }
                let _ = std::io::stdout().write_all(&output);
                let _ = std::io::stderr().write_all(&answer.stderr);
            } else {
                // Explicit diagnostics can inspect exactly the host projection
                // without repeating source discovery or a paid selection.
        if std::env::var_os("MUSTARD_SEARCH_TRACE").is_some_and(|value|value=="1" || value=="projection") {
                    let view=mustard_core::io::code_search::presentation::agent(&answer,&request,root);
                    answer.report["_trace_agent_view"]=json!(String::from_utf8_lossy(&view.stdout));
                }
                println!("{}", answer.report);
            }
            std::process::exit(answer.exit_code);
        }
        Err(reason) => {
            let fallback = if reason.starts_with("search-contract-")
                || reason.starts_with("search-intent-required")
                || reason == "search-unsupported-contract-version"
            {
                "correct-search-request; preserve original arguments and supply explicit intent and purpose"
            } else {
                "use-original-host-tool"
            };
            println!(
                "{}",
                json!({"ok":false,"reason":reason,"fallback":fallback,"executed":false,"remote_model_calls":0})
            );
            std::process::exit(2);
        }
    }
}
