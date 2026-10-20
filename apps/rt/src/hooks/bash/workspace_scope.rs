//! A conductor may inspect a wave checkout, but a Bash cd must not strand the
//! main session there. Workers keep their own cwd, and a command Mustard runs
//! in its own process has no host shell to strand. No checkout/reset is run.
//! The rtk swap comes before every wrapper, so the copy, the session and the
//! conductor's subshell all wrap the command rtk already shortened.
use super::lex;
use mustard_core::{
    domain::model::contract::{Ctx, HookInput, Verdict},
    io::{spec_events, wave_prompt, workspace},
    platform::code_tools::ToolRunner,
};
use std::path::Path;

/// `rtk` is the bundled rtk copy and the runner that asks it for the swap,
/// present only when the command runs in the host shell and the project keeps
/// the `rtk` option on.
pub(crate) fn isolate<R: ToolRunner>(input: &HookInput, ctx: &Ctx, verdict: Verdict, host_shell: bool, rtk: Option<(&Path, &R)>) -> Verdict {
    let verdict = normalize_test(input, ctx, verdict);
    let verdict = match rtk {
        Some((copy, runner)) if host_shell => super::rtk_swap::swap(input, verdict, copy, runner),
        _ => verdict,
    };
    let verdict = bind_identity(input, verdict);
    let verdict = crate::hooks::write::wave_copy::guard_bash(input, ctx, verdict);
    if input.is_subagent() || !host_shell {
        return verdict;
    }
    let (command, previous_note) = match &verdict {
        Verdict::Allow => (input.tool_input["command"].as_str(), None),
        Verdict::Rewrite { tool_input, note } => (tool_input["command"].as_str(), note.clone()),
        _ => return verdict,
    };
    let Some(command) = command else {
        return verdict;
    };
    let cwd = Path::new(input.cwd.as_deref().unwrap_or(&ctx.project_dir));
    let root = spec_events::spec_root(cwd);
    if !mustard_core::ProjectConfig::exists(&root) {
        return verdict;
    }
    let copies = wave_prompt::copies_dir(&root);
    let is_copy = |path: &Path| {
        path.starts_with(&copies) && workspace::linked_worktree_main(path).zip(root.canonicalize().ok()).is_some_and(|(main, root)| main == root)
    };
    let stranded = is_copy(cwd);
    let enters = lex::segments(command).iter().filter(|seg| seg.name() == "cd").any(|seg| {
        seg.args.iter().find(|arg| !arg.text.starts_with('-')).is_some_and(|arg| {
            // O caminho sai sem `.` e `..` antes de ser comparado com a pasta
            // das cópias: `<checkout>/../<cópias>/…` também entra numa cópia.
            let path = Path::new(&arg.text);
            is_copy(&crate::hooks::write::wave_copy::lexical(&if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) }))
        })
    });
    if !stranded && !enters {
        return verdict;
    }
    let quote = |path: &Path| format!("'{}'", wave_prompt::shown(path).replace('\'', "'\\''"));
    let isolated = if stranded {
        // Restore the conductor first, then run in the original cwd inside
        // the subshell. This also preserves failures under shell errexit.
        format!("cd -- {} && (\ncd -- {} &&\n{command}\n)", quote(&root), quote(cwd))
    } else {
        format!("(\n{command}\n)")
    };
    let note = match ctx.config.language().text_or_default() {
        mustard_core::SupportedLocale::PtBr => "O comando usa a cópia em um subshell; o orquestrador permanece no checkout principal.",
        mustard_core::SupportedLocale::EnUs => "The command uses the copy in a subshell; the conductor remains in the main checkout.",
    };
    let mut tool_input = match &verdict {
        Verdict::Rewrite { tool_input, .. } => tool_input.clone(),
        _ => input.tool_input.clone(),
    };
    tool_input["command"] = serde_json::json!(isolated);
    Verdict::Rewrite { tool_input, note: Some(previous_note.map_or_else(|| note.to_string(), |previous| format!("{previous}\n{note}"))) }
}

/// O comando nativo do Mustard fica sabendo quem o pediu: a sessão do host e,
/// dentro de um subagente, o agente. A busca confere o comando como esse agente.
fn bind_identity(input: &HookInput, verdict: Verdict) -> Verdict {
    let named = |id: &&str| !id.is_empty() && id.len() <= 160;
    let session = input.session_id.as_deref().filter(named).map(crate::shared::spec_state::shell_export);
    let agent = input.agent_id.as_deref().filter(named).map(crate::shared::search_gateway::agent_export);
    let prefix = session.unwrap_or_default() + &agent.unwrap_or_default();
    if prefix.is_empty() {
        return verdict;
    }
    let mut tool_input = match &verdict {
        Verdict::Allow => input.tool_input.clone(),
        Verdict::Rewrite { tool_input, .. } => tool_input.clone(),
        _ => return verdict,
    };
    let Some(command) = tool_input["command"].as_str() else {
        return verdict;
    };
    if !lex::segments(command).iter().any(|part| matches!(part.name(), "mustard-rt" | "mustard-rt.exe" | "mustard-boot" | "mustard-boot.cmd")) {
        return verdict;
    }
    if command.starts_with(&prefix) {
        return verdict;
    }
    tool_input["command"] = serde_json::json!(format!("{prefix}{command}"));
    let note = match verdict {
        Verdict::Rewrite { note, .. } => note,
        _ => None,
    };
    Verdict::Rewrite { tool_input, note }
}

fn normalize_test(input: &HookInput, ctx: &Ctx, verdict: Verdict) -> Verdict {
    let (mut tool_input, previous_note) = match &verdict {
        Verdict::Allow => (input.tool_input.clone(), None),
        Verdict::Rewrite { tool_input, note } => (tool_input.clone(), note.clone()),
        _ => return verdict,
    };
    let Some(command) = tool_input["command"].as_str() else {
        return verdict;
    };
    let normalized = crate::commands::review::qa_run::canonical_test_command(command);
    if normalized == command {
        return verdict;
    }
    let note = match ctx.config.language().text_or_default() {
        mustard_core::SupportedLocale::PtBr => {
            "Jest: a raiz foi escrita como --roots=<pasta> para preservar o caminho do teste como argumento separado."
        }
        mustard_core::SupportedLocale::EnUs => "Jest: wrote the root as --roots=<folder> to preserve the test path as a separate argument.",
    };
    tool_input["command"] = serde_json::json!(normalized);
    Verdict::Rewrite { tool_input, note: Some(previous_note.map_or_else(|| note.to_string(), |previous| format!("{previous}\n{note}"))) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::platform::code_tools::MachineRunner;
    use serde_json::json;
    const NO_RTK: Option<(&'static Path, &'static MachineRunner)> = None;
    #[test]
    fn native_cli_receives_the_exact_host_session_even_in_background() {
        let input: HookInput = serde_json::from_value(json!({"session_id":"current-session","tool_name":"Bash","tool_input":{"command":"mustard-rt run round --dispatch","run_in_background":true,"timeout":600_000}})).unwrap();
        let Verdict::Rewrite { tool_input, .. } = bind_identity(&input, Verdict::Allow) else {
            panic!("session propagation");
        };
        assert_eq!(tool_input["command"], "export MUSTARD_SESSION_ID='current-session'\nmustard-rt run round --dispatch");
        assert_eq!(tool_input["timeout"], 600_000);
        assert_eq!(tool_input["run_in_background"], true);
        assert!(matches!(bind_identity(&input, Verdict::Deny { reason: "protected".into() }), Verdict::Deny { .. }));
    }
    /// The project at `root` as a git repository with `mustard.json` and the
    /// copy of the `scope-test` spec as a linked worktree.
    fn project_with_copy(root: &Path) -> std::path::PathBuf {
        let git = |args: &[&str]| assert!(mustard_core::platform::git::run(root, args).ok, "git {args:?}");
        git(&["init", "-q"]);
        git(&[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-qm",
            "fixture",
        ]);
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        let copy = wave_prompt::slot_path(root, "scope-test", 0);
        git(&["worktree", "add", "--detach", copy.to_str().unwrap(), "HEAD"]);
        copy
    }

    #[test]
    fn copy_commands_preserve_conductor_cwd_exit_status_and_worker_identity() {
        let dir = tempfile::tempdir().unwrap();
        // macOS resolves /var to /private/var when sh starts in the temp tree.
        // Compare actual cwd paths, rather than two spellings of that tree.
        let canonical = dir.path().canonicalize().unwrap();
        let root = canonical.as_path();
        let git = |args: &[&str]| assert!(mustard_core::platform::git::run(root, args).ok, "git {args:?}");
        let copy = project_with_copy(root);
        let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(mustard_core::domain::model::contract::Trigger::PreToolUse));
        let jest: HookInput =
            serde_json::from_value(json!({"cwd":root,"tool_name":"Bash","tool_input":{"command":"jest --roots src src/example.spec.ts"}})).unwrap();
        let Verdict::Rewrite { tool_input, .. } = isolate(&jest, &ctx, Verdict::Allow, true, NO_RTK) else {
            panic!("Jest root normalization");
        };
        assert_eq!(tool_input["command"], "jest --roots=src src/example.spec.ts");
        assert!(matches!(isolate(&jest, &ctx, Verdict::Deny { reason: "existing protection".into() }, true, NO_RTK), Verdict::Deny { .. }));
        let mut input: HookInput = serde_json::from_value(
            json!({"cwd":root,"tool_name":"Bash","tool_input":{"command":format!("cd '{}' && pwd # comment",copy.display()),"timeout":1000}}),
        )
        .unwrap();
        let Verdict::Rewrite { tool_input, .. } = isolate(&input, &ctx, Verdict::Allow, true, NO_RTK) else {
            panic!("copy isolation");
        };
        assert_eq!(tool_input["timeout"], 1000);
        #[cfg(unix)]
        {
            let command = format!("{}\npwd", tool_input["command"].as_str().unwrap());
            let out = std::process::Command::new("sh").args(["-c", &command]).current_dir(root).output().unwrap();
            let lines = String::from_utf8(out.stdout).unwrap();
            assert_eq!(lines.lines().collect::<Vec<_>>(), vec![copy.to_str().unwrap(), root.to_str().unwrap()]);
            input.cwd = Some(copy.to_string_lossy().into_owned());
            input.tool_input["command"] = json!("exit 7");
            let Verdict::Rewrite { tool_input, .. } = isolate(&input, &ctx, Verdict::Allow, true, NO_RTK) else {
                panic!("root recovery");
            };
            let command = format!("{}\ncode=$?\npwd\nexit $code", tool_input["command"].as_str().unwrap());
            let out = std::process::Command::new("sh").args(["-c", &command]).current_dir(&copy).output().unwrap();
            assert_eq!(out.status.code(), Some(7));
            assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), root.to_str().unwrap());
            let marker = root.join("cwd-after-errexit");
            let command = format!("trap 'pwd > \"$1\"' EXIT\n{}", tool_input["command"].as_str().unwrap());
            let out =
                std::process::Command::new("sh").args(["-ec", &command, "fixture", marker.to_str().unwrap()]).current_dir(&copy).output().unwrap();
            assert_eq!(out.status.code(), Some(7));
            assert_eq!(std::fs::read_to_string(marker).unwrap().trim(), root.to_str().unwrap());
        }
        input.agent_id = Some("worker".into());
        assert!(matches!(isolate(&input, &ctx, Verdict::Allow, true, NO_RTK), Verdict::Allow));
        git(&["worktree", "remove", "--force", copy.to_str().unwrap()]);
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }

    /// The conductor's `cd` into the session temp folder, whose name carries
    /// the project path in hyphens, stays as written; a `cd` that reaches the
    /// copy through `..` from the main checkout runs in a subshell.
    #[test]
    fn the_conductor_cd_into_the_session_temp_folder_stays_as_written() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let root = canonical.as_path();
        let copy = project_with_copy(root);
        let temp = tempfile::tempdir().unwrap();
        // The folder name Claude Code gives a project: every character that is
        // not a letter or a digit becomes `-`, the `?` of a Windows `\\?\`
        // path included, which no Windows folder name may carry.
        let name: String = root.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let scratch = temp.path().join("claude-1000").join(name).join("session/scratchpad");
        std::fs::create_dir_all(&scratch).unwrap();
        let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(mustard_core::domain::model::contract::Trigger::PreToolUse));
        let mut input: HookInput =
            serde_json::from_value(json!({"cwd":root,"tool_name":"Bash","tool_input":{"command":format!("cd '{}' && ls", scratch.display())}})).unwrap();
        assert!(matches!(isolate(&input, &ctx, Verdict::Allow, true, NO_RTK), Verdict::Allow));
        // `<checkout>/../../<copy without the leading folder>`: the text starts
        // in the main checkout and the folder is the copy. A Windows `\\?\`
        // path takes `..` as a plain name, so the climb exists only on Linux
        // and macOS.
        if !cfg!(windows) {
            let normal = |path: &Path| path.components().filter(|part| matches!(part, std::path::Component::Normal(_))).count();
            let up: std::path::PathBuf = (0..normal(root)).map(|_| "..").collect();
            let below: std::path::PathBuf = copy.components().filter(|part| matches!(part, std::path::Component::Normal(_))).collect();
            let through_root = root.join(up).join(below);
            input.tool_input["command"] = json!(format!("cd '{}' && pwd", through_root.display()));
            let Verdict::Rewrite { tool_input, .. } = isolate(&input, &ctx, Verdict::Allow, true, NO_RTK) else { panic!("copy through ..") };
            assert!(tool_input["command"].as_str().unwrap().starts_with("(\n"), "{}", tool_input["command"]);
        }
        assert!(mustard_core::platform::git::run(root, &["worktree", "remove", "--force", copy.to_str().unwrap()]).ok);
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }
}
