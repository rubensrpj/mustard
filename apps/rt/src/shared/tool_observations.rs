//! Native tool-result provenance. No subprocesses, inference, result bodies or
//! extra context on each call; the wave detail lists receipts when requested.
use mustard_core::{
    domain::model::contract::HookInput,
    io::{
        observations::{self, Receipt},
        spec_events,
    },
};
use serde_json::Value;
use std::path::Path;

mod run_summary;

/// O aviso que o agente lê depois de um executor cujo resumo o pipe escondeu:
/// o recibo dele não prova como a execução acabou, e o mesmo executor sem o
/// pipe prova. `None` quando a saída mostra como ele acabou, ou quando o
/// código de saída já diz que falhou.
pub(crate) fn hidden_summary_note(input: &HookInput, lang: mustard_core::SupportedLocale) -> Option<String> {
    if input.tool_name.as_deref() != Some("Bash") {
        return None;
    }
    let command = input.tool_input.get("command").and_then(Value::as_str)?;
    let response = input.raw.get("tool_response")?;
    if response.get("exit_code").or_else(|| response.get("exitCode")).and_then(Value::as_i64).is_some_and(|code| code != 0) {
        return None;
    }
    (run_summary::read(command, response)? == run_summary::Summary::Hidden).then(|| {
        super::say::say("tool_observations.summary_hidden", lang, &[("{command}", &run_summary::without_pipe(command))])
    })
}

pub(crate) fn capture(
    root: &Path,
    host: &str,
    event: &str,
    input: &HookInput,
) -> Option<Result<(), &'static str>> {
    if event=="PostToolUse" && input.tool_name.as_deref()==Some("Read") {
        return skill_read(root,host,input);
    }
    if !["PostToolUse", "PostToolUseFailure"].contains(&event)
        || input.tool_name.as_deref() != Some("Bash")
    {
        return None;
    }
    let (Some(session), Some(call), Some(response)) = (
        input.session_id.as_deref(),
        input
            .raw
            .get("tool_use_id")
            .or_else(|| input.raw.get("call_id"))
            .and_then(Value::as_str),
        if event == "PostToolUseFailure" {
            input.raw.get("error")
        } else {
            input.raw.get("tool_response")
        },
    ) else {
        return None;
    };
    if session.is_empty()
        || session.len() > 256
        || call.is_empty()
        || call.len() > 256
        || response.is_null()
    {
        return None;
    }
    let command = input
        .tool_input
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if command.len() > 65_536 || runs_a_flow_step(command) {
        return None;
    }
    let root = spec_events::spec_root(root);
    let binding = super::host_observation::tool_binding(&root, host, input).map(|binding| (binding.spec, binding.wave, binding.dispatch)).or_else(|| {
        let copies = mustard_core::io::wave_prompt::copies_dir(&root);
        let marker = format!("{}/", mustard_core::io::wave_prompt::shown(&copies));
        let normalized = command.replace('\\', "/");
        let relative = input
            .tool_input
            .get("workdir")
            .and_then(Value::as_str)
            .or(input.cwd.as_deref())
            .and_then(|cwd| Path::new(cwd).strip_prefix(&copies).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"));
        // A command mentioning two copies cannot identify one dispatch. Do
        // not turn a best guess into provenance for an external assertion.
        let candidates: std::collections::BTreeSet<_> = relative
            .as_deref()
            .into_iter()
            .chain(
                normalized
                    .match_indices(&marker)
                    .map(|(start, _)| &normalized[start + marker.len()..]),
            )
            .filter_map(|path| {
                let mut parts = path.split('/');
                let spec = parts.next()?;
                let slot = parts.next()?.split(['\'', '"', ' ', '\t', '\n']).next()?;
                Some((spec.to_string(), slot.to_string()))
            })
            .collect();
        if candidates.len() != 1 {
            return None;
        }
        let (spec, slot) = candidates.into_iter().next()?;
        let log = spec_events::read(&spec_events::spec_file(&root, &spec).ok()?).ok()??;
        let send = log
            .last_by_wave("send")
            .into_values()
            .filter_map(|id| log.get(id))
            .find(|send| {
                send.str_field("copy")
                    .and_then(|p| Path::new(p).file_name())
                    .and_then(|p| p.to_str())
                    == Some(slot.as_str())
                    && send.str_field("host").is_none_or(|value| value == host)
            })?;
        Some((spec, send.wave()?, log.dispatch_position(send.id)))
    });
    let (spec, wave, dispatch) = binding.or_else(|| {
        if input.is_subagent() {return None;}
        crate::commands::spec_events::conversation::conversation_spec(&root,Some(session)).map(|spec|(spec,0,0))
    })?;
    let Ok(output) = serde_json::to_vec(response) else {
        return None;
    };
    if output.len() > 2_000_000 {
        return None;
    }
    let exit_code = response.get("exit_code").or_else(|| response.get("exitCode")).and_then(Value::as_i64);
    // Sem o código de saída do executor (o host não manda, ou o pipe entrega
    // só o do último programa), o resumo dele na saída decide o recibo.
    let summary = (event == "PostToolUse").then(|| run_summary::read(command, response)).flatten();
    let receipt = Receipt {
        version: 1,
        spec,
        dispatch,
        wave,
        host: host.into(),
        actor: observations::digest(
            serde_json::json!([session, input.agent_id])
                .to_string()
                .as_bytes(),
        ),
        call: observations::digest(call.as_bytes()),
        tool: "Bash".into(),
        source: kept_source(&super::secret::without_secrets(command)),
        input_sha256: observations::digest(command.as_bytes()),
        output_sha256: observations::digest(&output),
        exit_code,
        result_status: if event == "PostToolUseFailure" {
            if input.raw.get("is_interrupt") == Some(&Value::Bool(true)) {
                "interrupted"
            } else {
                "failed"
            }
        } else if exit_code.is_some_and(|code| code != 0)
            || response.get("isError").or_else(|| response.get("is_error"))
                == Some(&Value::Bool(true))
        {
            "failed"
        } else if let Some(summary) = summary {
            summary.status()
        } else {
            "returned"
        }
        .into(),
        observed_at: chrono::Utc::now().to_rfc3339(),
    };
    Some(
        observations::record(&root, &receipt)
            .map(|_| ())
            .map_err(|error| {
                if error.contains("conflicting-tool-result") {
                    "observation-conflicting-tool-result"
                } else {
                    "observation-store-unavailable"
                }
            }),
    )
}

/// The text a receipt keeps of `command`: what the hooks put ahead of it, up
/// to 1,000 characters, and then 300 characters of the agent's own command,
/// so the prefix of a wave copy never takes the room of the command.
fn kept_source(command: &str) -> String {
    let (prefix, written) = command.split_at(observations::command_start(command));
    prefix.chars().take(1_000).chain(written.chars().take(300)).collect()
}

/// `true` when one of the commands of the line is the runtime calling a flow
/// step (`mustard-rt run …`), which records its own call. A command that only
/// names the program — a package test, a hook probe through `mustard-rt on …`
/// — is not such a call and keeps its receipt. Neither is the native search
/// the hook routed through the search gateway (`run search --request …`): the
/// agent wrote a `grep` or `rg`, and the rewrite must not cost its receipt.
fn runs_a_flow_step(command: &str) -> bool {
    crate::hooks::bash::lex::segments(command).iter().any(|segment| {
        matches!(segment.name(), "mustard-rt" | "mustard-rt.exe" | "mustard-boot" | "mustard-boot.cmd")
            && segment.args.first().is_some_and(|arg| arg.text == "run")
            && !routed_native_search(&segment.args)
    })
}

/// `true` for the arguments `run search … --request <envelope>`, the form in
/// which the hook hands a native search to the gateway.
fn routed_native_search(args: &[crate::hooks::bash::lex::Word]) -> bool {
    args.get(1).is_some_and(|arg| arg.text == "search")
        && args.windows(2).any(|pair| {
            pair[0].text == "--request"
                && mustard_core::domain::code_search::Request::from_json(&pair[1].text).is_ok()
        })
}

fn skill_read(root:&Path,host:&str,input:&HookInput)->Option<Result<(),&'static str>> {
    // Só o arquivo de uma skill pede a ligação: as outras leituras do agente
    // param aqui.
    if input.tool_input["file_path"].as_str().and_then(|path|Path::new(path).file_name()).is_none_or(|name|name!="SKILL.md") {return None;}
    // O envio já veio lido com a ligação: o arquivo de eventos não é lido de novo.
    let super::host_observation::Binding{spec,wave,dispatch,send}=super::host_observation::tool_binding(root,host,input)?;
    let call=input.raw.get("tool_use_id").or_else(||input.raw.get("call_id"))?.as_str()?;
    let actor=input.agent_id.as_deref()?;
    let path=input.tool_input["file_path"].as_str()?;
    let content=input.raw.pointer("/tool_response/file/content")?.as_str()?;
    let sha=observed_skill(&send,path,content)?;
    let receipt=Receipt {version:1,spec,dispatch,wave,host:host.into(),actor:actor.into(),call:call.into(),tool:"Read".into(),source:path.into(),
        input_sha256:observations::digest(input.tool_input.to_string().as_bytes()),output_sha256:sha,exit_code:None,
        result_status:"succeeded".into(),observed_at:chrono::Utc::now().to_rfc3339()};
    Some(observations::record(root,&receipt).map(|_|()).map_err(|_|"skill-read-receipt-failed"))
}

fn observed_skill(send:&mustard_core::domain::spec_events::SpecEvent,path:&str,content:&str)->Option<String> {
    if content.len()>64_000 {return None;}
    let sha=observations::digest(content.as_bytes());
    send.fields.get("skills")?.as_array()?.iter().any(|skill|skill["path"]==path && skill["sha"]==sha).then_some(sha)
}

pub(crate) fn list(root: &Path, spec: &str, dispatch: Option<u64>) -> Vec<Value> {
    dispatch
        .and_then(|dispatch| observations::list(root, spec, dispatch).ok())
        .unwrap_or_default()
}

#[cfg(test)] mod tests {
    use super::*;use serde_json::json;
    #[test] fn a_partial_or_different_skill_read_is_not_confirmed() {
        let text="Reuse the existing transaction helper.\n";
        let log=mustard_core::domain::spec_events::parse_log(&json!({"id":1,"type":"send","wave":1,
            "skills":[{"path":"/project/.claude/skills/storage/SKILL.md","sha":observations::digest(text.as_bytes())}]}).to_string());
        let send=&log.events[0];let path="/project/.claude/skills/storage/SKILL.md";
        assert!(observed_skill(send,path,text).is_some());
        assert!(observed_skill(send,path,"Reuse the existing").is_none());
        assert!(observed_skill(send,"/other/SKILL.md",text).is_none());
    }

    /// A fresh project whose main conversation is bound to spec `x`, before
    /// any wave runs.
    fn bound_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        crate::commands::spec_events::write::record_open(dir.path(), "x", "fix/x", "dev").unwrap();
        crate::shared::context::session::bind_session_spec(&dir.path().to_string_lossy(), "survey", "x");
        dir
    }

    /// The payload the host sends after `command` returned in `cwd`.
    fn ran(command: &str, call: &str, cwd: &Path) -> HookInput {
        HookInput {
            session_id: Some("survey".into()),
            tool_name: Some("Bash".into()),
            tool_input: json!({"command": command}),
            cwd: Some(cwd.to_string_lossy().into()),
            raw: json!({"tool_use_id": call, "tool_response": {"stdout": "ok", "exit_code": 0}}),
            ..Default::default()
        }
    }

    #[test]
    fn a_cargo_proof_naming_the_runtime_package_gets_a_receipt() {
        let dir = bound_project();
        let root = dir.path();
        let copy = mustard_core::io::wave_prompt::copies_dir(root).join("x").join("c");
        let commands = ["rtk cargo test -p mustard-rt --lib x", "target/debug/mustard-rt on PreToolUse"];
        for (place, cwd) in [("project", root), ("copy", copy.as_path())] {
            for (n, command) in commands.iter().enumerate() {
                let input = ran(command, &format!("{place}-{n}"), cwd);
                assert_eq!(capture(root, "claude", "PostToolUse", &input), Some(Ok(())), "{command} in the {place}");
            }
        }
        assert_eq!(list(root, "x", Some(0)).len(), 4);
    }

    #[test]
    fn every_native_command_in_the_copy_gets_a_receipt_even_when_the_hook_routes_it_to_the_search() {
        let dir = bound_project();
        let root = dir.path();
        let copy = mustard_core::io::wave_prompt::copies_dir(root).join("x").join("c");
        let shown = copy.display();
        let written = [
            format!("cd {shown} && grep -n \"export function quote\" web/src/quote.ts"),
            format!("cd {shown} && cargo test -p mustard-rt --bin mustard-rt -- host_observation 2>&1 | grep -E \"FAILED|test result\""),
            format!("cd {shown} && cp src/a.rs /tmp/a.rs.bak && sed -i 's/old/new/' src/a.rs"),
        ];
        let ctx = mustard_core::domain::model::contract::Ctx {
            project_dir: root.to_string_lossy().into(),
            ..Default::default()
        };
        let mut routed = 0;
        for (n, command) in written.iter().enumerate() {
            // The command the host runs, and reports afterwards, is the one the
            // hook rewrote before the call.
            let before = HookInput { tool_input: json!({"command": command, "description": "find capture"}), ..ran(command, "", root) };
            let ran_command = match crate::shared::search_gateway::before_bash(&before, &ctx) {
                Some(mustard_core::domain::model::contract::Verdict::Rewrite { tool_input, .. }) => {
                    routed += 1;
                    tool_input["command"].as_str().unwrap().to_string()
                }
                _ => command.clone(),
            };
            let input = ran(&ran_command, &format!("agent-{n}"), root);
            assert_eq!(capture(root, "claude", "PostToolUse", &input), Some(Ok(())), "{ran_command}");
        }
        assert_eq!(routed, 1, "the plain grep goes through the search");
        assert_eq!(list(root, "x", Some(0)).len(), written.len());
    }

    /// A command led to the wave copy and named with the session and the
    /// agent keeps on the page 80 characters of what the agent wrote.
    #[test]
    fn a_receipt_keeps_the_agent_command_after_the_hook_prefix() {
        let dir = bound_project();
        let root = dir.path();
        let copy = mustard_core::io::wave_prompt::copies_dir(root).join("x").join("a-long-copy-folder-name-for-the-wave-".repeat(3));
        let written = format!("grep -rn \"export function quote\" web/src/{}.ts packages/ui/src 2>&1 | head -40", "quote-".repeat(20));
        let written: String = written.chars().take(150).collect();
        let command = format!(
            "cd -- '{}' && (\nexport MUSTARD_SESSION_ID='{}'\nexport MUSTARD_AGENT_ID='{}'\n{written}\n)",
            copy.display(), "s".repeat(36), "a".repeat(17)
        );
        assert!(command.len() - written.len() > 230, "the prefix leaves less than 80 of the old 300 characters");
        assert_eq!(capture(root, "claude", "PostToolUse", &ran(&command, "agent-long", root)), Some(Ok(())));
        let page = list(root, "x", Some(0));
        let shown = page[0]["source"].as_str().unwrap();
        assert_eq!(shown, format!("{}…", written.chars().take(80).collect::<String>()), "{shown}");
    }

    /// Sem o código de saída, o resumo do executor decide o recibo: o verde
    /// passa e o vermelho falha. O pipe que tira o resumo deixa o recibo sem
    /// veredito, e o gancho manda rodar o executor sem o pipe.
    #[test]
    fn a_runner_summary_decides_the_receipt_when_the_host_sends_no_exit_code() {
        use mustard_core::domain::model::contract::{Trigger, Verdict};
        let dir = bound_project();
        let root = dir.path();
        let piped = "cargo test -j 1 -p mustard-rt --lib -- tool_observations 2>&1 | grep -E \"test result|FAILED\"";
        let grep_ok = "cd /c && rtk cargo test -p x --lib -- 'a b' 2>&1 | grep ok";
        let answered = |command: &str, call: &str, stdout: &str| HookInput {
            hook_event_name: Some("PostToolUse".into()),
            raw: json!({"tool_use_id": call, "tool_response": {"stdout": stdout, "stderr": "", "interrupted": false, "isImage": false}}),
            ..ran(command, "", root)
        };
        let green = answered(piped, "green", "test result: ok. 30 passed; 0 failed; 0 ignored");
        let red = answered(piped, "red", "test result: FAILED. 29 passed; 1 failed");
        let hidden = answered(grep_ok, "hidden", "test a ... ok\ntest b ... ok");
        for input in [&green, &red, &hidden] {
            assert_eq!(capture(root, "claude", "PostToolUse", input), Some(Ok(())));
        }
        let mut statuses: Vec<_> = list(root, "x", Some(0)).iter().map(|row| (row["result_status"].clone(), row["exit_code"].clone())).collect();
        statuses.sort_by_key(|(status, _)| status.to_string());
        assert_eq!(statuses, [(json!("failed"), Value::Null), (json!("passed"), Value::Null), (json!("summary-hidden"), Value::Null)]);
        let note = |input: &HookInput| match crate::dispatch::run_event(Trigger::from_event_name("PostToolUse"), input).verdict {
            Verdict::Inject { context } => Some(context),
            _ => None,
        };
        let told = note(&hidden).expect("the hidden summary is told");
        assert!(told.contains("`cargo test -p x --lib -- 'a b'`"), "{told}");
        assert_eq!(note(&green), None);
    }

    #[test]
    fn a_runtime_flow_call_gets_no_receipt() {
        let dir = bound_project();
        let root = dir.path();
        for (n, command) in [
            "mustard-rt run round --dispatch",
            "cd /p && MUSTARD_RT_DELEGATED=1 ~/.local/bin/mustard-boot run read request-1 | head",
            "mustard-rt run search --shell-output --intent \"where\" --purpose implement -- rg -n capture apps",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(capture(root, "claude", "PostToolUse", &ran(command, &format!("flow-{n}"), root)), None, "{command}");
        }
        assert!(list(root, "x", Some(0)).is_empty());
    }
}
