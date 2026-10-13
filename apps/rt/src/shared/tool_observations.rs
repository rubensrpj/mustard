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
    if command.contains("mustard-rt") || command.len() > 65_536 {
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

pub(crate) fn list(root: &Path, spec: &str, dispatch: Option<u64>) -> Vec<Value> {
    dispatch
        .and_then(|dispatch| observations::list(root, spec, dispatch).ok())
        .unwrap_or_default()
}
