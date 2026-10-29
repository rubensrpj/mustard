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
    let input = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "cwd": dir.to_str().expect("utf-8 path"),
        "session_id": "command-guard-test",
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
    let (_folder, program) = own_program();
    let empty = TempDir::new().expect("empty config folder");
    let mut child = binary::isolated(&program)
        .args(["on", "PreToolUse"])
        .current_dir(dir)
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("MUSTARD_WORKSPACE_ROOT")
        .env("CLAUDE_CONFIG_DIR", config.unwrap_or(empty.path()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mustard-rt");
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

/// A commit message and a pending-item title that only name a base deletion
/// pass; the real command is refused. A forced folder delete is left to the
/// native deny list of the settings.
#[test]
fn quoted_text_passes_and_a_real_base_delete_is_blocked() {
    let dir = project_with(r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#);
    for command in [
        r#"git commit -m "limpa: git branch -D dev""#,
        r#"mustard-rt run pending --add --title "rodar git branch -D dev depois""#,
        "rm -rf pasta",
    ] {
        let out = run_guard(dir.path(), command);
        assert!(refusal(&out).is_none(), "{command} must pass: {out}");
    }
    let out = run_guard(dir.path(), "git branch -D dev");
    let reason = refusal(&out).unwrap_or_else(|| panic!("git branch -D dev must be refused: {out}"));
    assert!(reason.starts_with("Comando barrado: apagar a branch de integração `dev`"), "{reason}");
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
    assert_eq!(updated["run_in_background"], true, "the request keeps the agent's own fields: {out}");
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
    let dir = project_with(r#"{"language":{"text":"en-US"},"git":{"flow":{"*":"dev","dev":"main"}}}"#);
    let out = run_guard(dir.path(), "git branch -D dev");
    let reason = refusal(&out).unwrap_or_else(|| panic!("git branch -D dev must be refused: {out}"));
    assert!(reason.starts_with("Command blocked: deleting the integration branch `dev`"), "{reason}");
}

/// A recusa do filtro no projeto de idioma `language`, pelo binário.
fn filter_refusal(command: &str, language: &str) -> String {
    let dir = project_with(&format!(r#"{{"language":{{"text":"{language}"}}}}"#));
    refusal(&run_guard(dir.path(), command)).unwrap_or_else(|| panic!("{command} must be refused in {language}"))
}

/// Se a recusa `reason` manda gravar a resposta num arquivo: pela palavra,
/// num dos dois idiomas, ou por um redirecionamento.
fn offers_a_file(reason: &str) -> bool {
    let lower = reason.to_lowercase();
    lower.contains("arquivo") || lower.contains("file") || reason.contains(" > ") || reason.contains(" >> ")
}

/// O filtro da resposta do Mustard é recusado nos dois idiomas, e a recusa
/// termina no caminho daquele comando: o resumo pronto do bloco grande, a
/// leitura por termo dos blocos que a aceitam, ou o comando sozinho, com ou sem a
/// razão de resposta curta. Nenhuma das quatro recusas manda gravar a
/// resposta num arquivo, e a do bloco grande não sugere a leitura por termo.
#[test]
fn the_filter_refusal_points_to_the_summary_and_never_to_a_file() {
    use mustard_core::platform::i18n::{Locale, translate};

    for (language, locale) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        let summary = translate("runtime_output.summary", locale).replace("{block}", "metrics").replace("{summary}", "calls");
        for (command, way) in [
            ("mustard-rt run read metrics --spec x | grep calls", summary.as_str()),
            ("mustard-rt run read notes --spec x | head -20", translate("runtime_output.term", locale)),
            ("mustard-rt run round --spec x | grep next", translate("runtime_output.alone_short", locale)),
            ("mustard-rt run panel | grep waves", translate("runtime_output.alone", locale)),
        ] {
            let reason = filter_refusal(command, language);
            assert!(reason.ends_with(way), "{command} ends on its way in {language}: {reason}");
            assert!(!offers_a_file(&reason), "the refusal never offers a file: {reason}");
            if way == summary {
                assert!(!reason.contains("--term"), "the summary is the only way given: {reason}");
            }
        }
    }
}

/// O filtro da resposta da rodada é recusado nos dois idiomas, e a recusa
/// diz para rodar o comando sozinho, porque a resposta já vem curta. Ela não
/// sugere a leitura por termo; a ausência de arquivo nela e nas outras
/// recusas do filtro é provada no teste do resumo, acima.
#[test]
fn the_round_filter_refusal_says_to_run_it_alone() {
    use mustard_core::platform::i18n::{Locale, translate};

    for (language, locale) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        let reason = filter_refusal("mustard-rt run round --spec x | grep next", language);
        assert!(reason.ends_with(translate("runtime_output.alone_short", locale)), "the refusal says to run it alone: {reason}");
        assert!(!reason.contains("--term"), "the round has no read by term: {reason}");
    }
}

/// A resposta do Mustard passada a um interpretador é recusada por qualquer
/// nome que a gramática do interpretador reconhece: o lançador do Python no
/// Windows, o Python rodado pelo `uv run` e o nome com `.exe`, pelo cano e
/// por substituição. A recusa diz para rodar o comando sozinho.
#[test]
fn a_runtime_answer_fed_to_any_interpreter_name_is_refused() {
    use mustard_core::platform::i18n::{Locale, translate};

    let alone = translate("runtime_output.alone", Locale::PtBr);
    for command in [
        r#"mustard-rt run panel | py -c "import sys; print(sys.stdin.read()[:100])""#,
        r#"mustard-rt run panel | uv run python -c "import sys; print(sys.stdin.read()[:100])""#,
        r#"mustard-rt run panel | python.exe -c "import sys; print(sys.stdin.read()[:100])""#,
        "py summarize.py <(mustard-rt run panel)",
        "uv run python summarize.py <(mustard-rt run panel)",
    ] {
        let reason = filter_refusal(command, "pt-BR");
        assert!(reason.ends_with(alone), "{command} ends on running it alone: {reason}");
    }
}
