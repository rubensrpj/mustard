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
#[path = "support/mod.rs"]
mod support;

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

/// Um projeto com duas cópias de onda: a `own`, que o envio da onda 1 deu ao
/// agente `wave-agent` da sessão `wave-session`, e a `other`, de outra vaga.
#[cfg(unix)]
struct WaveCopies {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    own: std::path::PathBuf,
    other: std::path::PathBuf,
}

#[cfg(unix)]
fn wave_copies() -> WaveCopies {
    use std::process::Command;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let git = |args: &[&str]| assert!(Command::new("git").args(args).current_dir(&root).output().unwrap().status.success(), "git {args:?}");
    git(&["init", "-q"]);
    git(&["-c", "user.name=fixture", "-c", "user.email=fixture@example.com", "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", "fixture"]);
    std::fs::write(root.join("mustard.json"), r#"{"ai":{"fallback":false,"vectors":false}}"#).unwrap();
    std::fs::create_dir(root.join(".claude")).unwrap();
    support::copies_leave_with_the_test(&root);
    let [own, other] = [0, 1].map(|slot| mustard_core::io::wave_prompt::slot_path(&root, "search-scope", slot));
    for copy in [&own, &other] {
        git(&["worktree", "add", "--detach", copy.to_str().unwrap(), "HEAD"]);
        std::fs::create_dir(copy.join("src")).unwrap();
        std::fs::write(copy.join("src/a.py"), "def quartz(): return 1\n").unwrap();
    }
    let log = mustard_core::io::spec_events::spec_file(&root, "search-scope").unwrap();
    let send = serde_json::json!({"wave":1,"role":"wave","lines":1,"chars":1,"mustard":"test","copy":own,"host":"claude","text":"task"});
    let sent = mustard_core::io::spec_events::write(&log, "send", send.as_object().unwrap().clone(), &[]).unwrap();
    let dispatch = mustard_core::io::spec_events::read(&log).unwrap().unwrap().dispatch_position(sent.id);
    let digest = |text: &str| {
        let mut hash = mustard_core::io::sha256::Sha256::new();
        hash.update(text.as_bytes());
        hash.hex_digest()
    };
    let bindings = root.join(".claude/mustard/host-transcripts");
    std::fs::create_dir_all(&bindings).unwrap();
    std::fs::write(
        bindings.join(format!("claude-{}-{}.json", digest("wave-session"), digest("wave-agent"))),
        serde_json::json!({"host":"claude","spec":"search-scope","wave":1,"dispatch":dispatch}).to_string(),
    )
    .unwrap();
    WaveCopies { _dir: dir, root, own, other }
}

/// O que a chamada do Bash roda, de dentro da cópia `own`, depois do gancho,
/// pedida pelo agente `agent` ou, sem ele, pelo condutor: o código de saída,
/// a saída e, para as mensagens, o comando que rodou com tudo o que ele
/// escreveu.
#[cfg(unix)]
fn in_own_copy(copies: &WaveCopies, agent: Option<&str>, command: &str) -> (Option<i32>, String, String) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_mustard-rt"));
    let input = serde_json::json!({"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":copies.own,
        "session_id":"wave-session","agent_id":agent,"tool_input":{"command":command}});
    let mut hook = Command::new(binary).args(["on", "PreToolUse"]).current_dir(&copies.own).env("CLAUDE_PROJECT_DIR", &copies.root)
        .env("MUSTARD_RT_DELEGATED", "1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    hook.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
    let output = hook.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let value = if output.stdout.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&output.stdout).unwrap() };
    assert_ne!(value["hookSpecificOutput"]["permissionDecision"], "deny", "{value}");
    let rewritten = value["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap_or(command).to_owned();
    assert!(rewritten.contains("mustard-rt run search"), "{rewritten}");
    // Só o comando que o gancho devolveu diz quem busca: nada vem do
    // ambiente de quem roda o teste.
    let path = format!("{}:{}", binary.parent().unwrap().display(), std::env::var("PATH").unwrap_or_default());
    let out = Command::new("sh").args(["-c", &rewritten]).current_dir(&copies.own).env("PATH", path)
        .env("MUSTARD_RT_DELEGATED", "1").env_remove("MUSTARD_SESSION_ID").env_remove("MUSTARD_AGENT_ID").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let detail = format!("{rewritten}\n{stdout}{}", String::from_utf8_lossy(&out.stderr));
    (out.status.code(), stdout, detail)
}

/// O agente de onda busca dentro da própria cópia, pelo gancho ou pelo comando
/// de busca, e recebe a resposta da busca nativa, sem ser recusado como se
/// fosse o condutor preso na cópia.
#[cfg(unix)]
#[test]
fn a_wave_agent_search_inside_its_copy_runs() {
    let copies = wave_copies();
    for command in [
        "grep -n quartz src/a.py",
        "mustard-rt run search --shell-output --intent 'Find quartz' --purpose implement -- grep -n quartz src/a.py",
    ] {
        let (code, stdout, detail) = in_own_copy(&copies, Some("wave-agent"), command);
        assert_eq!(code, Some(0), "{detail}");
        assert!(stdout.contains("def quartz"), "{detail}");
    }
}

/// Quem a busca não sabe nomear também busca numa cópia de onda: a ferramenta
/// de busca, que roda o programa sem shell, e o condutor com o shell na cópia.
/// A busca roda no próprio processo e não tem shell a prender na cópia, então
/// não devolve o subshell do condutor, que a trava da cópia recusaria ao
/// agente de onda. O shell do condutor segue voltando ao checkout principal
/// pelo gancho.
#[cfg(unix)]
#[test]
fn a_search_in_a_wave_copy_runs_for_a_caller_without_identity() {
    let copies = wave_copies();
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_mustard-rt"));
    let tool = std::process::Command::new(binary)
        .args(["run", "search", "--root"])
        .arg(&copies.own)
        .args(["--shell-output", "--intent", "Find quartz", "--purpose", "implement", "--", "grep", "-n", "quartz", "src/a.py"])
        .current_dir(&copies.root)
        .env("MUSTARD_RT_DELEGATED", "1")
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("MUSTARD_AGENT_ID")
        .output()
        .unwrap();
    let detail = format!("{}{}", String::from_utf8_lossy(&tool.stdout), String::from_utf8_lossy(&tool.stderr));
    assert_eq!(tool.status.code(), Some(0), "{detail}");
    assert!(String::from_utf8_lossy(&tool.stdout).contains("def quartz"), "{detail}");
    let (code, stdout, detail) = in_own_copy(&copies, None, "grep -n quartz src/a.py");
    assert!(detail.starts_with(&format!("cd -- '{}' && (", copies.root.display())), "{detail}");
    assert_eq!(code, Some(0), "{detail}");
    assert!(stdout.contains("def quartz"), "{detail}");
}

/// A busca do mesmo agente apontada para a cópia de outra onda segue recusada.
#[cfg(unix)]
#[test]
fn a_wave_agent_search_in_another_waves_copy_is_refused() {
    let copies = wave_copies();
    let command = format!(
        "mustard-rt run search --root '{}' --shell-output --intent 'Find quartz' --purpose implement -- grep -n quartz src/a.py",
        copies.other.display()
    );
    let (code, stdout, detail) = in_own_copy(&copies, Some("wave-agent"), &command);
    assert_eq!(code, Some(2), "{detail}");
    assert!(!stdout.contains("def quartz"), "{detail}");
}

/// Uma busca que acha uma linha de mais de um megabyte, num JSON de uma linha
/// só, responde abaixo do teto da conversa, de 25.000 bytes. Cada linha de
/// mais de 1.000 bytes mostra só o começo e diz quantos bytes dela ficaram de
/// fora, e as linhas do TypeScript que vêm depois dela aparecem. As que não
/// cabem saem do fim, e a nota final diz o tamanho da resposta inteira, de
/// que linha em diante cortou e o comando que grava o resultado inteiro num
/// arquivo; esse comando entrega a busca nativa byte a byte.
#[cfg(unix)]
#[test]
fn a_search_hitting_a_line_over_a_megabyte_answers_under_the_conversation_cap() {
    use std::process::Command;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("assets")).unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("mustard.json"), r#"{"ai":{"fallback":false,"vectors":false}}"#).unwrap();
    let vocab: Vec<String> = (0..80_000).map(|n| format!("\"tok{n}\":{n}")).collect();
    let tokenizer = format!("{{\"note\":\"needle\",\"vocab\":{{{}}}}}", vocab.join(","));
    assert!(tokenizer.len() > 1_000_000, "the JSON line must pass a megabyte: {}", tokenizer.len());
    std::fs::write(root.join("assets/tokenizer.json"), &tokenizer).unwrap();
    let hits: Vec<String> = (0..30)
        .map(|n| {
            let line = format!("export const needle{n:02} = \"{}\";", "a".repeat(1_500));
            std::fs::write(root.join(format!("src/hit{n:02}.ts")), format!("{line}\n")).unwrap();
            format!("src/hit{n:02}.ts:1:{line}\n")
        })
        .collect();
    let native = format!("assets/tokenizer.json:1:{tokenizer}\n{}", hits.concat());
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_mustard-rt"));
    let search = ["rg", "-n", "--sort", "path", "needle", "assets", "src"];
    let out = Command::new(binary)
        .args(["run", "search", "--root"])
        .arg(&root)
        .arg("--shell-output")
        .arg("--")
        .args(search)
        .env("MUSTARD_RT_DELEGATED", "1")
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("MUSTARD_AGENT_ID")
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let detail = format!("{} bytes\n{}\n{}", stdout.len(), &stdout[..stdout.len().min(4_000)], String::from_utf8_lossy(&out.stderr));
    assert_eq!(out.status.code(), Some(0), "{detail}");
    assert!(stdout.len() <= 25_000, "{detail}");

    let lines: Vec<&str> = stdout.lines().collect();
    let kept = lines.iter().position(|line| line.starts_with("# Answer cut")).unwrap_or_else(|| panic!("no cut note: {detail}"));
    let whole: Vec<&str> = native.lines().collect();
    assert!(kept > 1 && kept < whole.len(), "the TypeScript hits after the long line show, and the tail is left out: {detail}");
    for (shown, line) in lines[..kept].iter().zip(&whole) {
        assert_eq!(*shown, format!("{} [line cut: {} more bytes]", &line[..1_000], line.len() - 1_000), "{detail}");
    }
    assert_eq!(
        lines[kept],
        format!(
            "# Answer cut to fit the conversation cap of 25000 bytes; the whole answer has {} bytes in {} lines. \
             Lines over 1000 bytes showing only their start: {kept}. Lines {} to {} are left out.",
            native.len(),
            whole.len(),
            kept + 1,
            whole.len()
        ),
        "{detail}"
    );
    let quoted = search.iter().map(|arg| format!("'{arg}'")).collect::<Vec<_>>().join(" ");
    let rest = format!("# Whole native result: mustard-rt run search --root '{}' --raw -- {quoted} > <file>", root.display());
    assert_eq!(lines[kept + 1..], [rest.as_str()], "{detail}");

    let saved = tempfile::tempdir().unwrap();
    let file = saved.path().join("whole.txt");
    let command = rest.strip_prefix("# Whole native result: ").unwrap().replace("<file>", &file.display().to_string());
    let path = format!("{}:{}", binary.parent().unwrap().display(), std::env::var("PATH").unwrap_or_default());
    let rerun = Command::new("sh").args(["-c", &command]).env("PATH", path).env("MUSTARD_RT_DELEGATED", "1")
        .env_remove("MUSTARD_SESSION_ID").env_remove("MUSTARD_AGENT_ID").output().unwrap();
    assert_eq!(rerun.status.code(), Some(0), "{command}: {}", String::from_utf8_lossy(&rerun.stderr));
    assert!(std::fs::read(&file).unwrap() == native.as_bytes(), "{command}: the saved result is not the native search");
}
