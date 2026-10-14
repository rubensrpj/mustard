//! Bind wave writes to the exact native dispatch, never to inherited cwd.
use mustard_core::{
    domain::model::contract::{Ctx, HookInput, Verdict},
    io::{spec_events, wave_prompt},
};
use std::path::Path;

pub(crate) fn check(root: &Path, input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    if !input.is_subagent() {
        return None;
    }
    let host = crate::shared::host_observation::caller_host(root, input.session_id.as_deref());
    let binding = crate::shared::host_observation::tool_binding(root, host, input);
    let supplied = input.file_path()?;
    let cwd = Path::new(input.cwd.as_deref().unwrap_or(&ctx.project_dir));
    let path = Path::new(&supplied);
    let path = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    let mut absolute = std::path::PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => { absolute.pop(); }
            std::path::Component::CurDir => {}
            other => absolute.push(other.as_os_str()),
        }
    }
    let copies = wave_prompt::copies_dir(root);
    if !absolute.starts_with(root) && !absolute.starts_with(&copies) {
        return None;
    }
    let native_wave = input.agent_type.as_deref().is_some_and(|kind| kind == "mustard-wave" || kind.ends_with(":mustard-wave"));
    if binding.is_none() && !native_wave && !absolute.starts_with(&copies) {
        return None;
    }
    let Some((spec, wave, _)) = binding else {
        return Some(Verdict::Deny{reason:"Mustard: cannot identify this wave's native dispatch. Read the recorded wave request before editing; do not choose a copy from inherited cwd.".into()});
    };
    let log = spec_events::spec_file(root, &spec).ok().and_then(|path| spec_events::read(&path).ok().flatten())?;
    let send = log.last_by_wave("send").get(&wave).and_then(|id| log.get(*id))?;
    let copy = Path::new(send.str_field("copy")?);
    let parent = absolute.ancestors().find(|path| path.exists()).and_then(|path| path.canonicalize().ok());
    let expected = copy.canonicalize().ok();
    if parent.zip(expected).is_some_and(|(parent, copy)| parent.starts_with(copy)) {
        return None;
    }
    Some(Verdict::Deny {
        reason: format!(
            "Mustard: wave {wave} of {spec} may write only in {}. The requested path is in another checkout. Use the exact copy from the native request.",
            wave_prompt::shown(copy)
        ),
    })
}

pub(crate) fn guard_bash(input: &HookInput, ctx: &Ctx, verdict: Verdict) -> Verdict {
    let mut adjusted = input.clone();
    let previous_note = match &verdict {
        Verdict::Allow => None,
        Verdict::Rewrite { tool_input, note } => {
            adjusted.tool_input = tool_input.clone();
            note.clone()
        }
        _ => return verdict,
    };
    match bash(&adjusted, ctx) {
        Some(Verdict::Rewrite { tool_input, note }) => Verdict::Rewrite {
            tool_input,
            note: match (previous_note, note) {
                (Some(first), Some(second)) => Some(format!("{first}\n{second}")),
                (first, second) => first.or(second),
            },
        },
        Some(other) => other,
        None => verdict,
    }
}

fn bash(input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    if !input.is_subagent() {
        return None;
    }
    let root = Path::new(&ctx.project_dir);
    let host = crate::shared::host_observation::caller_host(root, input.session_id.as_deref());
    let (spec, wave, _) = crate::shared::host_observation::tool_binding(root, host, input)?;
    let log = spec_events::spec_file(root, &spec).ok().and_then(|path| spec_events::read(&path).ok().flatten())?;
    let send = log.last_by_wave("send").get(&wave).and_then(|id| log.get(*id))?;
    let expected = Path::new(send.str_field("copy")?);
    let real = expected.canonicalize().ok()?;
    let cwd = Path::new(input.cwd.as_deref().unwrap_or(&ctx.project_dir));
    let command = input.tool_input["command"].as_str()?;
    let copies = wave_prompt::copies_dir(root);
    for segment in crate::hooks::bash::lex::segments(command).iter().filter(|segment| segment.name() == "cd") {
        if let Some(arg) = segment.args.iter().find(|arg| !arg.text.starts_with('-')) {
            let given = Path::new(&arg.text);
            let path = if given.is_absolute() { given.to_path_buf() } else { cwd.join(given) };
            if (path.starts_with(&copies) || path.starts_with(root)) && path.canonicalize().is_ok_and(|path| !path.starts_with(&real)) {
                // A pasta de cópias pode morar dentro do projeto: ela decide primeiro.
                let another_copy = path.starts_with(&copies);
                let shown = wave_prompt::shown(expected);
                let reason = match ctx.config.language().text_or_default() {
                    mustard_core::SupportedLocale::PtBr => format!(
                        "Mustard: a onda {wave} usa somente {shown}. O comando tenta entrar {}; rode-o de dentro da cópia, sem `cd` para fora dela.",
                        if another_copy { "na cópia de outra onda" } else { "no checkout principal" }
                    ),
                    mustard_core::SupportedLocale::EnUs => format!(
                        "Mustard: wave {wave} uses only {shown}. The command tries to enter {}; run it inside the copy, without a `cd` out of it.",
                        if another_copy { "another wave's copy" } else { "the main checkout" }
                    ),
                };
                return Some(Verdict::Deny { reason });
            }
        }
    }
    if cwd.canonicalize().is_ok_and(|cwd| cwd.starts_with(&real)) {
        return None;
    }
    let mut tool_input = input.tool_input.clone();
    tool_input["command"] = serde_json::json!(format!("cd -- '{}' && (\n{command}\n)", wave_prompt::shown(expected).replace('\'', "'\\''")));
    Some(Verdict::Rewrite { tool_input, note: Some(format!("Mustard: o comando da onda {wave} foi direcionado à cópia registrada no pedido.")) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    /// O agente `worker`, despachado para a onda 1 da spec `work` com a cópia
    /// `a`, escrevendo de dentro da cópia `b`, de outra vaga.
    fn dispatched_worker(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, Ctx, HookInput) {
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let a = wave_prompt::slot_path(root, "work", 0);
        let b = wave_prompt::slot_path(root, "work", 1);
        for path in [&a, &b] {
            std::fs::create_dir_all(path).unwrap();
        }
        let path = spec_events::spec_file(root, "work").unwrap();
        let sent = spec_events::write(
            &path,
            "send",
            json!({"wave":1,"role":"wave","lines":1,"chars":1,"mustard":"test","copy":a,"host":"claude","text":"task"}).as_object().unwrap().clone(),
            &[],
        )
        .unwrap();
        let log = spec_events::read(&path).unwrap().unwrap();
        let folder = root.join(".claude/mustard/host-transcripts");
        std::fs::create_dir_all(&folder).unwrap();
        let digest = |s: &str| {
            let mut hash = mustard_core::io::sha256::Sha256::new();
            hash.update(s.as_bytes());
            hash.hex_digest()
        };
        std::fs::write(
            folder.join(format!("claude-{}-{}.json", digest("session"), digest("worker"))),
            json!({"host":"claude","spec":"work","wave":1,"dispatch":log.dispatch_position(sent.id)}).to_string(),
        )
        .unwrap();
        let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(mustard_core::domain::model::contract::Trigger::PreToolUse));
        let input:HookInput=serde_json::from_value(json!({"agent_id":"worker","agent_type":"mustard-wave","session_id":"session","cwd":b,"tool_name":"Write","tool_input":{"file_path":b.join("file.rs")}})).unwrap();
        (a, b, ctx, input)
    }

    #[test]
    fn dispatched_agent_cannot_write_or_enter_another_waves_copy() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (a, b, ctx, mut input) = dispatched_worker(root);
        assert!(matches!(check(root, &input, &ctx), Some(Verdict::Deny { .. })));
        input.tool_input["file_path"] = json!(a.join("new/folder/file.rs"));
        assert!(check(root, &input, &ctx).is_none(), "new directories inside the assigned copy are allowed");
        input.tool_input["file_path"] = json!(a.join("new/../../b/file.rs"));
        assert!(matches!(check(root, &input, &ctx), Some(Verdict::Deny { .. })), "missing parent directories cannot hide a traversal into another copy");
        input.tool_name = Some("Bash".into());
        input.tool_input = json!({"command":"pwd"});
        assert!(matches!(guard_bash(&input, &ctx, Verdict::Allow), Verdict::Rewrite { .. }), "inherited cwd is repaired before execution");
        input.tool_input = json!({"command":format!("cd '{}' && write-something",b.display())});
        assert!(matches!(guard_bash(&input, &ctx, Verdict::Allow), Verdict::Deny { .. }));
        assert!(matches!(guard_bash(&input, &ctx, Verdict::Deny { reason: "existing protection".into() }), Verdict::Deny { .. }));
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }

    /// O `cd` para o checkout principal é recusado dizendo que vai ao checkout
    /// principal, e o `cd` para outra cópia, dizendo que vai à cópia de outra
    /// onda.
    #[test]
    fn a_wave_agent_entering_the_main_checkout_is_told_it_is_the_main_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (a, b, ctx, mut input) = dispatched_worker(root);
        input.cwd = Some(a.to_string_lossy().into_owned());
        input.tool_name = Some("Bash".into());
        let mut refusal = |command: String| {
            input.tool_input = json!({ "command": command });
            match guard_bash(&input, &ctx, Verdict::Allow) {
                Verdict::Deny { reason } => reason,
                other => panic!("{command}: {other:?}"),
            }
        };
        let main = refusal(format!("cd -- '{}' && (\ncd -- '{}' &&\ngrep -n quartz src\n)", root.display(), a.display()));
        assert!(main.contains("no checkout principal") && !main.contains("outra onda"), "{main}");
        let other = refusal(format!("cd '{}' && pwd", b.display()));
        assert!(other.contains("na cópia de outra onda"), "{other}");
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }
}
