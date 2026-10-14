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
    let binding = super::host_observation::tool_binding(&root, host, input).or_else(|| {
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
        source: super::secret::without_secrets(command)
            .chars()
            .take(300)
            .collect(),
        input_sha256: observations::digest(command.as_bytes()),
        output_sha256: observations::digest(&output),
        exit_code: response
            .get("exit_code")
            .or_else(|| response.get("exitCode"))
            .and_then(Value::as_i64),
        result_status: if event == "PostToolUseFailure" {
            if input.raw.get("is_interrupt") == Some(&Value::Bool(true)) {
                "interrupted"
            } else {
                "failed"
            }
        } else if response
            .get("exit_code")
            .or_else(|| response.get("exitCode"))
            .and_then(Value::as_i64)
            .is_some_and(|code| code != 0)
            || response.get("isError").or_else(|| response.get("is_error"))
                == Some(&Value::Bool(true))
        {
            "failed"
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

/// `true` when one of the commands of the line is the runtime calling a flow
/// step (`mustard-rt run …`), which records its own call. A command that only
/// names the program — a package test, a hook probe through `mustard-rt on …`
/// — is not such a call and keeps its receipt.
fn runs_a_flow_step(command: &str) -> bool {
    crate::hooks::bash::lex::segments(command).iter().any(|segment| {
        matches!(segment.name(), "mustard-rt" | "mustard-rt.exe" | "mustard-boot" | "mustard-boot.cmd")
            && segment.args.first().is_some_and(|arg| arg.text == "run")
    })
}

fn skill_read(root:&Path,host:&str,input:&HookInput)->Option<Result<(),&'static str>> {
    let (spec,wave,dispatch)=super::host_observation::tool_binding(root,host,input)?;
    let call=input.raw.get("tool_use_id").or_else(||input.raw.get("call_id"))?.as_str()?;
    let actor=input.agent_id.as_deref()?;
    let path=input.tool_input["file_path"].as_str()?;
    let content=input.raw.pointer("/tool_response/file/content")?.as_str()?;
    let log=spec_events::read(&spec_events::spec_file(root,&spec).ok()?).ok()??;
    let send=log.events.iter().rev().find(|e|e.event_type=="send" && e.wave()==Some(wave) && log.dispatch_position(e.id)==dispatch)?;
    let sha=observed_skill(send,path,content)?;
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
    fn a_runtime_flow_call_gets_no_receipt() {
        let dir = bound_project();
        let root = dir.path();
        for (n, command) in [
            "mustard-rt run round --dispatch",
            "cd /p && MUSTARD_RT_DELEGATED=1 ~/.local/bin/mustard-boot run read request-1 | head",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(capture(root, "claude", "PostToolUse", &ran(command, &format!("flow-{n}"), root)), None, "{command}");
        }
        assert!(list(root, "x", Some(0)).is_empty());
    }
}
