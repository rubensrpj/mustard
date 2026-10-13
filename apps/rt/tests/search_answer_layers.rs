// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A resposta da busca se monta na parte compartilhada.
//!
//! O comando de busca e o gancho da ferramenta de busca dão a mesma resposta,
//! e a peça que a monta mora em `shared`, que os dois usam. Se `shared`
//! importasse do comando ou do gancho, ou um deles do outro, os dois
//! caminhos voltariam a depender um do outro em círculo. O teste lê o código
//! dos arquivos, sem os comentários, e confere as importações.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

/// As linhas de código de `source`, sem os comentários, que trazem `path`.
fn reaching<'s>(source: &'s str, path: &str) -> Vec<&'s str> {
    source.lines().map(str::trim_start).filter(|line| !line.starts_with("//") && line.contains(path)).collect()
}

/// A porta, a busca por palavra e a triagem não importam de `commands::map`
/// nem de `hooks`; o comando não importa de `hooks`; o gancho não importa de
/// `commands::map`. Os arquivos citados existem: o nome que sumisse deixaria
/// o teste verde sem conferir nada.
#[test]
fn the_search_answer_is_built_in_shared_without_command_and_hook_reaching_each_other() {
    let src = manifest_dir::manifest_dir().join("src");
    let shared = ["shared/search_door.rs", "shared/word_search.rs", "shared/triage_view.rs"];
    let checks = shared
        .iter()
        .map(|file| (*file, ["crate::commands::map", "crate::hooks"].as_slice()))
        .chain([
            ("commands/map.rs", ["crate::hooks"].as_slice()),
            ("hooks/write/write_gate.rs", ["crate::commands::map"].as_slice()),
        ]);
    let mut reached = Vec::new();
    for (file, forbidden) in checks {
        let source = std::fs::read_to_string(src.join(file)).unwrap_or_else(|e| panic!("{file} must be readable: {e}"));
        assert!(source.lines().count() > 50, "{file} is too short to be the file it names");
        for path in forbidden {
            reached.extend(reaching(&source, path).into_iter().map(|line| format!("{file}: {line}")));
        }
    }
    assert!(reached.is_empty(), "the search answer reaches across the layers: {reached:#?}");
}


#[cfg(unix)]
#[test]
fn shell_gateway_preserves_pipe_filters_cwd_conditions_and_exit_codes() {
    use std::io::Write;
    use std::process::{Command,Stdio};
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.py"),"def quartz(): return 1\ndef zircon(): return 2\n").unwrap();
    std::fs::write(root.join("mustard.json"),r#"{"ai":{"fallback":false,"vectors":false}}"#).unwrap();
    let binary=std::path::Path::new(env!("CARGO_BIN_EXE_mustard-rt"));
    let search_path=format!("{}:{}",binary.parent().unwrap().display(),std::env::var("PATH").unwrap_or_default());
    for command in ["grep -En 'quartz|zircon' src/a.py | grep return", "cd src && grep quartz a.py | head -n 1", "grep missing src/a.py || grep zircon src/a.py", "false && grep quartz src/a.py", "cat src/a.py | grep quartz"] {
        let input=serde_json::json!({"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":root,"session_id":"shell-parity","tool_input":{"command":command}});
        let mut hook=Command::new(binary).args(["on","PreToolUse"]).current_dir(root).env("CLAUDE_PROJECT_DIR",root)
            .env("MUSTARD_RT_DELEGATED","1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        hook.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
        let output=hook.wait_with_output().unwrap();assert!(output.status.success());
        let value=if output.stdout.is_empty() {serde_json::Value::Null} else {serde_json::from_slice(&output.stdout).unwrap()};
        assert_ne!(value["hookSpecificOutput"]["permissionDecision"],"deny","{value}");
        let rewritten=value["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap_or(command).to_owned();
        if command.starts_with("cat") {assert_eq!(rewritten,command,"a stream filter is not a new file search");}
        else {assert!(rewritten.contains("mustard-rt run search"),"{rewritten}");}
        let run=|script:&str|std::process::Command::new("sh").args(["-c",script]).current_dir(root)
            .env("PATH",&search_path).env("MUSTARD_RT_DELEGATED","1").output().unwrap();
        let original=run(command);let gateway=run(&rewritten);
        assert_eq!(gateway.status.code(),original.status.code(),"{rewritten}: {}",String::from_utf8_lossy(&gateway.stderr));
        assert_eq!(gateway.stdout,original.stdout,"{rewritten}: {}",String::from_utf8_lossy(&gateway.stderr));
    }
}
