// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The command guard from the outside: the binary answers a `PreToolUse` of
//! the Bash tool the way the Claude Code sends it, always in a temporary
//! project, never in the real one.

#[path = "support/binary.rs"]
mod binary;

use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use serde_json::Value;
use tempfile::TempDir;

/// Roda o gancho de antes da ferramenta Bash para um comando, a partir de
/// `dir`, e devolve o que ele imprimiu.
fn run_guard(dir: &Path, command: &str) -> String {
    run_guard_in(dir, command, "command-guard-test")
}

/// The same, for the conversation `session` of the host.
fn run_guard_in(dir: &Path, command: &str, session: &str) -> String {
    let input = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "cwd": dir.to_str().expect("utf-8 path"),
        "session_id": session,
        "tool_input": { "command": command }
    });
    run_hook(dir, &input, None)
}

/// The program under test in a folder of its own, under the target's
/// temporary folder: a hard link of the built `mustard-rt`, or a copy where
/// the link cannot be made. The hook looks for the bundled rtk beside the
/// running program, and nothing that sits beside the build — an `rtk` left
/// in `target/debug` on one machine — may reach the test. The folder leaves
/// with the returned guard.
fn own_program() -> (TempDir, std::path::PathBuf) {
    let built = Path::new(env!("CARGO_BIN_EXE_mustard-rt"));
    let folder = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).expect("folder of the program");
    let program = folder.path().join(built.file_name().expect("program name"));
    if std::fs::hard_link(built, &program).is_err() {
        std::fs::copy(built, &program).expect("copy of the program");
    }
    (folder, program)
}

/// Run `mustard-rt on PreToolUse` with the whole `input`, from `dir`, with
/// Claude Code's config folder moved to `config`, or to an empty folder: the
/// plugin installed on the machine never takes part.
fn run_hook(dir: &Path, input: &Value, config: Option<&Path>) -> String {
    run_hook_with_temp(dir, input, config, None)
}

/// O mesmo, com a pasta temporária do programa trocada por `temp`, quando ela
/// vem: é onde a recusa manda gravar a resposta de um comando cortado.
fn run_hook_with_temp(dir: &Path, input: &Value, config: Option<&Path>, temp: Option<&Path>) -> String {
    let (_folder, program) = own_program();
    let empty = TempDir::new().expect("empty config folder");
    let mut command = binary::isolated(&program);
    command
        .args(["on", "PreToolUse"])
        .current_dir(dir)
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("MUSTARD_WORKSPACE_ROOT")
        .env("CLAUDE_CONFIG_DIR", config.unwrap_or(empty.path()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(temp) = temp {
        // `TMPDIR` no Unix; `TMP` e `TEMP` no Windows.
        command.env("TMPDIR", temp).env("TMP", temp).env("TEMP", temp);
    }
    let mut child = command.spawn().expect("spawn mustard-rt");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(input.to_string().as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait mustard-rt");
    assert!(
        output.status.success(),
        "mustard-rt exited {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The refusal the hook printed, when it refused.
fn refusal(stdout: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(stdout.trim()).ok()?;
    if parsed.pointer("/hookSpecificOutput/permissionDecision")?.as_str()? != "deny" {
        return None;
    }
    let reason = parsed.pointer("/hookSpecificOutput/permissionDecisionReason").and_then(Value::as_str);
    Some(reason.unwrap_or_default().to_string())
}

/// Separa `line` em palavras como o shell faz com aspas simples: o espaço
/// separa fora das aspas, `'...'` guarda o texto como está e a barra
/// invertida fora das aspas protege o caractere seguinte, como no `'\''` de um
/// nome com aspa simples. Assim o caminho volta sem as aspas que o terminal
/// pede.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let (mut started, mut quoted) = (false, false);
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if !quoted => {
                if let Some(next) = chars.next() {
                    word.push(next);
                    started = true;
                }
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// Os arquivos em que o comando pronto `line` grava, sem as aspas.
fn redirect_targets(line: &str) -> Vec<String> {
    let words = shell_words(line);
    words.windows(2).filter(|pair| pair[0] == ">").map(|pair| pair[1].clone()).collect()
}

/// O arquivo em que o comando pronto, na última linha da recusa `reason`,
/// grava a resposta; sem as aspas.
fn written_file(reason: &str) -> String {
    let ready = reason.lines().last().unwrap_or_default();
    redirect_targets(ready).pop().unwrap_or_else(|| panic!("the ready command writes to a file: {ready}"))
}

/// O nome do arquivo em `path`, sem a pasta.
fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

fn project_with(config: &str) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("mustard.json"), config).expect("write mustard.json");
    dir
}

/// Every line of every `.ndjson` file under `root`.
fn ndjson_lines(root: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return lines;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.is_dir() {
            lines.extend(ndjson_lines(&path));
        } else if path.extension().is_some_and(|e| e == "ndjson") {
            let body = std::fs::read_to_string(&path).unwrap_or_default();
            lines.extend(body.lines().map(str::to_string));
        }
    }
    lines
}

/// A commit message and a pending-item title that only name a dangerous
/// command pass; the real command is refused.
#[test]
fn quoted_text_passes_and_a_real_delete_is_blocked() {
    let dir = TempDir::new().expect("tempdir");
    for command in [
        r#"git commit -m "limpa: rm -rf build antigo""#,
        r#"mustard-rt run pending --add --title "rodar rm -rf target antes do build""#,
    ] {
        let out = run_guard(dir.path(), command);
        assert!(refusal(&out).is_none(), "{command} must pass: {out}");
    }
    let out = run_guard(dir.path(), "rm -rf pasta");
    let reason = refusal(&out).unwrap_or_else(|| panic!("rm -rf pasta must be refused: {out}"));
    assert!(reason.starts_with("Comando barrado: apagar pasta à força"), "{reason}");
}

/// Outside a Mustard project the guard hands the command back untouched:
/// the rtk swap acts only where `mustard.json` keeps it on.
#[test]
fn an_unprefixed_command_outside_a_project_is_not_rewritten() {
    let dir = TempDir::new().expect("tempdir");
    let out = run_guard(dir.path(), "git status");
    assert!(!out.contains("updatedInput"), "{out}");
    assert!(refusal(&out).is_none(), "{out}");
}

/// A plugin install under `config` whose `bin/rtk` answers `rtk rewrite` the
/// way rtk 0.49 does: `rtk` in front of the command, exit 3. Answers the
/// path of that rtk copy.
#[cfg(unix)]
fn plugin_with_rtk(config: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = config.join("plugin/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let rtk = bin.join("rtk");
    std::fs::write(&rtk, "#!/bin/sh\n[ \"$1\" = rewrite ] || exit 9\nprintf 'rtk %s\\n' \"$3\"\nexit 3\n").unwrap();
    std::fs::set_permissions(&rtk, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(config.join("plugins")).unwrap();
    let registry = serde_json::json!({"plugins":{"mustard@local":[{"version":"0.0.1","installPath":config.join("plugin")}]}});
    std::fs::write(config.join("plugins/installed_plugins.json"), registry.to_string()).unwrap();
    rtk
}

/// The conductor's command, in a project with the rtk option on, comes out
/// with one swap only: the rtk copy that ships with Mustard, by its whole
/// path, in front of the command.
#[cfg(unix)]
#[test]
fn the_conductor_command_gets_one_rtk_swap_through_the_bundled_copy() {
    let dir = project_with("{}");
    let config = TempDir::new().expect("config");
    let rtk = plugin_with_rtk(config.path());
    let input = serde_json::json!({"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":dir.path(),"session_id":"conductor",
        "tool_input":{"command":"git status","timeout":600_000}});
    let out: Value = serde_json::from_str(run_hook(dir.path(), &input, Some(config.path())).trim()).expect("json");
    assert_eq!(out["hookSpecificOutput"]["updatedInput"]["command"], format!("'{}' git status", rtk.display()), "{out}");

    std::fs::write(dir.path().join("mustard.json"), r#"{"rtk":false}"#).unwrap();
    let off = run_hook(dir.path(), &input, Some(config.path()));
    assert!(!off.contains("updatedInput"), "the rtk option off keeps the command: {off}");
}

/// A wave agent outside its copy runs `cargo test` once, inside the copy,
/// through the bundled rtk copy, with the 600-second ceiling and in the
/// foreground: the rtk swap comes before the copy's wrapper, never racing it.
#[cfg(unix)]
#[test]
fn a_wave_agent_cargo_test_runs_once_in_its_copy_through_the_bundled_rtk() {
    use mustard_core::io::{sha256::Sha256, spec_events, wave_prompt};
    let dir = project_with("{}");
    let root = dir.path();
    let config = TempDir::new().expect("config");
    let rtk = plugin_with_rtk(config.path());
    let copy = wave_prompt::slot_path(root, "work", 0);
    std::fs::create_dir_all(&copy).unwrap();
    let path = spec_events::spec_file(root, "work").unwrap();
    let send = serde_json::json!({"wave":1,"role":"wave","lines":1,"chars":1,"mustard":"test","copy":copy,"host":"claude","text":"task"});
    let sent = spec_events::write(&path, "send", send.as_object().unwrap().clone(), &[]).unwrap();
    let log = spec_events::read(&path).unwrap().unwrap();
    let digest = |text: &str| {
        let mut hash = Sha256::new();
        hash.update(text.as_bytes());
        hash.hex_digest()
    };
    let bindings = root.join(".claude/mustard/host-transcripts");
    std::fs::create_dir_all(&bindings).unwrap();
    std::fs::write(
        bindings.join(format!("claude-{}-{}.json", digest("session"), digest("worker"))),
        serde_json::json!({"host":"claude","spec":"work","wave":1,"dispatch":log.dispatch_position(sent.id)}).to_string(),
    )
    .unwrap();
    let input = serde_json::json!({"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":root,"session_id":"session",
        "agent_id":"worker","agent_type":"mustard-wave","tool_input":{"command":"cargo test","run_in_background":true}});
    let out: Value = serde_json::from_str(run_hook(root, &input, Some(config.path())).trim()).expect("json");
    let updated = &out["hookSpecificOutput"]["updatedInput"];
    assert_eq!(
        updated["command"],
        format!("cd -- '{}' && (\n'{}' cargo test\n)", wave_prompt::shown(&copy), rtk.display()),
        "{out}"
    );
    assert_eq!(updated["timeout"], 600_000);
    assert!(updated.get("run_in_background").is_none(), "{out}");
    std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
}

#[test]
fn the_guard_leaves_no_rewrite_event_behind() {
    let dir = TempDir::new().expect("tempdir");
    for command in ["git status", "rm -rf pasta", "cargo build"] {
        run_guard(dir.path(), command);
    }
    let rewrites: Vec<String> =
        ndjson_lines(dir.path()).into_iter().filter(|line| line.contains("rtk-rewrite")).collect();
    assert!(rewrites.is_empty(), "{rewrites:?}");
}

#[test]
fn a_flow_base_is_protected_and_other_branches_are_not() {
    let dir = project_with(r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#);
    let out = run_guard(dir.path(), "git branch -D dev");
    let reason = refusal(&out).unwrap_or_else(|| panic!("deleting dev must be refused: {out}"));
    assert!(reason.contains("`dev`"), "{reason}");
    let out = run_guard(dir.path(), "git branch -D feature/x");
    assert!(refusal(&out).is_none(), "{out}");
}

#[test]
fn the_refusal_follows_the_project_language() {
    let dir = project_with(r#"{"language":{"text":"en-US"}}"#);
    let out = run_guard(dir.path(), "rm -rf pasta");
    let reason = refusal(&out).unwrap_or_else(|| panic!("rm -rf pasta must be refused: {out}"));
    assert!(reason.starts_with("Command blocked: deleting a folder by force"), "{reason}");
}

/// O comando pronto da recusa grava a resposta de cada etapa do Mustard num
/// arquivo próprio, com o nome da etapa, na pasta temporária, fora do
/// repositório; a etapa de ajuda, que passa sozinha, sai intacta, com o corte
/// dela. Antes a sugestão gravava todas as etapas no mesmo `arquivo`, na
/// pasta do projeto — que o `git add -A` de um pre-commit leva ao commit —, e
/// reescrevia também a ajuda.
#[test]
fn the_ready_command_writes_each_answer_to_its_own_file_outside_the_project() {
    let dir = project_with(r#"{"language":{"text":"pt-BR"}}"#);
    let ready = |command: &str| {
        let reason = refusal(&run_guard(dir.path(), command)).unwrap_or_else(|| panic!("{command} must be refused"));
        reason.lines().last().unwrap_or_default().to_string()
    };

    let mixed = ready("mustard-rt run answer --help | head && mustard-rt run resume | head");
    assert!(mixed.starts_with("mustard-rt run answer --help | head && mustard-rt run resume > "), "the help step stays as written: {mixed}");
    let resume = redirect_targets(&mixed);
    assert_eq!(resume.len(), 1, "{mixed}");
    assert!(resume[0].contains("resume"), "{mixed}");

    let two = ready("mustard-rt run read backlog --spec x 2>&1 | head; mustard-rt run read criteria --spec x");
    let files = redirect_targets(&two);
    assert_eq!(files.len(), 2, "{two}");
    assert_ne!(files[0], files[1], "one file per step: {two}");
    assert!(files[0].contains("backlog") && files[1].contains("criteria"), "the name says the step: {two}");
    for file in resume.iter().chain(&files) {
        let path = Path::new(file);
        assert!(path.is_absolute() && !path.starts_with(dir.path()), "{file} must sit outside the project");
        assert!(path.starts_with(std::env::temp_dir()), "{file} must sit in the temporary folder");
    }
}

/// Duas conversas que cortam o mesmo comando ao mesmo tempo não dividem o
/// arquivo do comando pronto: o id da sessão que o host mandou entra no nome.
#[test]
fn the_ready_command_names_its_file_by_the_session_that_asked() {
    let dir = project_with(r#"{"language":{"text":"pt-BR"}}"#);
    let ready = |session: &str| {
        let reason = refusal(&run_guard_in(dir.path(), "mustard-rt run resume | head -80", session)).expect("the cut must be refused");
        written_file(&reason)
    };
    assert_eq!(file_name(&ready("conversa-a1")), "mustard-resume-conversa-a1.txt");
    assert_eq!(file_name(&ready("conversa-b2")), "mustard-resume-conversa-b2.txt");
}

/// Dois agentes da mesma conversa, que o host identifica pelo mesmo id de
/// sessão, não dividem o arquivo do comando pronto: o id do agente entra no
/// nome, ao lado do da sessão, e a conversa principal fica só com o da sessão.
#[test]
fn the_ready_command_names_its_file_by_the_agent_that_asked() {
    let dir = project_with(r#"{"language":{"text":"pt-BR"}}"#);
    let ready = |agent: Option<&str>| {
        let mut input = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "cwd": dir.path().to_str().expect("utf-8 path"),
            "session_id": "conversa-c3",
            "tool_input": { "command": "mustard-rt run resume | head -80" }
        });
        if let Some(agent) = agent {
            input["agent_id"] = Value::from(agent);
        }
        let reason = refusal(&run_hook(dir.path(), &input, None)).expect("the cut must be refused");
        file_name(&written_file(&reason))
    };
    assert_eq!(ready(None), "mustard-resume-conversa-c3.txt");
    assert_eq!(ready(Some("agente-a1")), "mustard-resume-conversa-c3.agente-a1.txt");
    assert_eq!(ready(Some("agente-b2")), "mustard-resume-conversa-c3.agente-b2.txt");
}

/// Com um espaço na pasta temporária, o comando pronto traz o caminho do
/// arquivo inteiro, entre aspas, mesmo quando o comando passa de 200
/// caracteres: cortar o texto no meio do caminho tirava a aspa do fim e
/// deixava um comando que o terminal não fecha.
#[test]
fn the_ready_command_keeps_its_quoted_path_whole_in_a_temporary_folder_with_a_space() {
    let dir = project_with(r#"{"language":{"text":"pt-BR"}}"#);
    let root = TempDir::new().expect("tempdir");
    let temp = root.path().join("with space");
    std::fs::create_dir(&temp).expect("temporary folder with a space");
    // O caminho do arquivo começa perto do caractere 165 e acaba depois do 200.
    let command = format!("echo {} && mustard-rt run resume | head -5", "x".repeat(132));
    let input = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "cwd": dir.path().to_str().expect("utf-8 path"),
        "session_id": "conversa-d4",
        "tool_input": { "command": command }
    });
    let reason = refusal(&run_hook_with_temp(dir.path(), &input, None, Some(&temp))).expect("the cut must be refused");

    let ready = reason.lines().last().unwrap_or_default();
    assert!(ready.len() > 200, "the ready command must go past the old cut: {ready}");
    assert!(ready.ends_with(".txt'"), "the closing quote must stay: {ready}");
    let file = written_file(&reason);
    assert_eq!(Path::new(&file), temp.join("mustard-resume-conversa-d4.txt"), "{ready}");
}
