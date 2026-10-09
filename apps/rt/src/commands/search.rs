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
    let result = (|| {
        let mut request: Request = match request {
            Some(text) if text.len() <= 64 * 1024 => {
                serde_json::from_str(text).map_err(|e| e.to_string())?
            }
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
        Ok((answer, request)) => {
            if raw || shell_output {
                let output = if shell_output {
                    mustard_core::io::code_search::presentation::agent(&answer, &request, root).stdout
                } else {
                    answer.stdout
                };
                let _ = std::io::stdout().write_all(&output);
                let _ = std::io::stderr().write_all(&answer.stderr);
            } else {
                println!("{}", answer.report);
            }
            std::process::exit(answer.exit_code);
        }
        Err(reason) => {
            println!(
                "{}",
                json!({"ok":false,"reason":reason,"fallback":"use-original-host-tool","executed":false,"remote_model_calls":0})
            );
            std::process::exit(2);
        }
    }
}
