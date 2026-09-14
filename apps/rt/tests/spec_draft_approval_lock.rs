// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A trava da aprovação vale de ponta a ponta.
//!
//! Uma spec em plano, com o `state` semeado à mão no `spec.ndjson`: o portão
//! de escrita barra o código do projeto; "Ajustar" na pergunta não muda nada;
//! "Aprovar" grava a aprovação, e o código passa.
//!
//! Os ganchos rodam como processo, porque são privados da biblioteca, com o
//! `CLAUDE_PROJECT_DIR` na pasta temporária.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const SPEC: &str = "cadastro";
const QUESTION: &str = "Aprovar esta spec?";

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?} failed");
}

/// Roda o gancho `event` com `payload`, com as variáveis `env` a mais, e
/// devolve a saída dele.
fn hook(root: &Path, event: &str, payload: Value, env: &[(&str, &str)]) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mustard-rt"));
    command
        .args(["on", event])
        .current_dir(root)
        .env("CLAUDE_PROJECT_DIR", root)
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("MUSTARD_APPROVAL_MODE")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().expect("spawn mustard-rt");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = write!(stdin, "{payload}");
    }
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0), "a hook always exits 0");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Editar o código do projeto é barrado?
fn edit_is_blocked(root: &Path) -> bool {
    let out = hook(
        root,
        "PreToolUse",
        json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Write",
            "tool_input": { "file_path": root.join("src/lib.rs").to_str().unwrap(), "content": "x" },
            "session_id": "s-lock",
            "cwd": root.to_str().unwrap()
        }),
        &[],
    );
    out.contains("\"deny\"")
}

/// Responde a pergunta de aprovação com `choice`, com as variáveis `env` a
/// mais.
fn answer(root: &Path, choice: &str, env: &[(&str, &str)]) -> String {
    hook(
        root,
        "PostToolUse",
        json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "AskUserQuestion",
            "tool_input": { "questions": [{ "question": QUESTION, "options": [
                { "label": "Aprovar" }, { "label": "Ajustar" }
            ] }] },
            "tool_response": { "questions": [], "answers": { QUESTION: choice } },
            "session_id": "s-lock",
            "cwd": root.to_str().unwrap()
        }),
        env,
    )
}

/// Um projeto com o fluxo `dev`/`main`, parado na branch da spec, e a spec em
/// plano: o `state` com a fase e a branch, semeado à mão no `spec.ndjson`.
fn spec_in_plan() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["checkout", "-q", "-b", "dev"]);
    std::fs::write(root.join("README.md"), "oi\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "init"]);
    git(root, &["checkout", "-q", "-b", &format!("feature/{SPEC}")]);
    let spec_dir = root.join(".claude").join("spec").join(SPEC);
    std::fs::create_dir_all(&spec_dir).unwrap();
    let line = json!({
        "v": 1, "id": 1, "at": "2026-09-14T10:00:00-03:00", "type": "state", "author": "binary",
        "phase": "plan", "branch": format!("feature/{SPEC}")
    });
    std::fs::write(spec_dir.join("spec.ndjson"), format!("{line}\n")).unwrap();
    tmp
}

#[test]
fn a_spec_in_plan_blocks_code_until_the_user_approves() {
    let tmp = spec_in_plan();
    let root = tmp.path();

    // Antes da aprovação, o código do projeto é barrado.
    assert!(edit_is_blocked(root), "a spec in plan blocks code before the approval");

    // "Ajustar" não aprova: continua barrado.
    answer(root, "Ajustar", &[]);
    assert!(edit_is_blocked(root), "adjusting keeps the lock");

    // "Aprovar" grava a aprovação, e o código passa.
    let said = answer(root, "Aprovar", &[]);
    assert!(said.contains("/clear"), "the witness suggests /clear: {said}");
    assert!(!edit_is_blocked(root), "after the approval the code is open");
}

/// A variável do modo de aprovação do comando antigo não muda mais a
/// testemunha: com qualquer valor, e com o `meta.json` de uma spec Full sem o
/// `.clarified`, o "Aprovar" aprova. Roda num processo à parte, para a
/// variável não chegar aos outros testes.
#[test]
fn the_approval_mode_variable_no_longer_changes_the_witness() {
    for mode in ["strict", "warn", "off"] {
        let tmp = spec_in_plan();
        let root = tmp.path();
        let spec_dir = root.join(".claude").join("spec").join(SPEC);
        std::fs::write(spec_dir.join("meta.json"), r#"{"scope":"full (wave plan)","stage":"Plan"}"#).unwrap();
        let said = answer(root, "Aprovar", &[("MUSTARD_APPROVAL_MODE", mode)]);
        assert!(said.contains("/clear"), "{mode}: the answer approves: {said}");
        assert!(!edit_is_blocked(root), "{mode}: after the approval the code is open");
    }
}

/// Os arquivos de texto de `dir`, com o caminho, em qualquer profundidade.
fn text_files(dir: &Path, out: &mut Vec<(std::path::PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            text_files(&path, out);
        } else if matches!(path.extension().and_then(|e| e.to_str()), Some("rs" | "md" | "ts" | "tsx"))
            && let Ok(body) = std::fs::read_to_string(&path)
        {
            out.push((path, body));
        }
    }
}

/// Toda gravação no arquivo de eventos de uma spec passa pela regra única da
/// mudança de fase: no código de produção, só o `commands/spec_events/write.rs`
/// chama o gravador do núcleo, e é lá que a regra confere cada gravação.
#[test]
fn every_state_write_goes_through_the_phase_rule() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in ["apps/rt/src", "apps/cli/src", "packages/core/src", "apps/dashboard/server/src"] {
        text_files(&repo.join(dir), &mut files);
    }
    let home = Path::new("apps/rt/src/commands/spec_events/write.rs");
    let writer = Path::new("packages/core/src/io/spec_events.rs");
    let mut hits = Vec::new();
    for (path, body) in &files {
        let relative = path.strip_prefix(&repo).unwrap_or(path);
        if relative == home || relative == writer || path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let production = body.split("#[cfg(test)]").next().unwrap_or_default();
        for call in [
            "spec_events::write(",
            "spec_events::write_at(",
            "store::write(",
            "store::write_at(",
            "write_then(",
            "write_at_then(",
            "write_guarded(",
        ] {
            if production.contains(call) {
                hits.push(format!("{} — {call}", relative.display()));
            }
        }
    }
    assert!(hits.is_empty(), "a spec event is written outside the phase rule:\n{}", hits.join("\n"));
    let rule = std::fs::read_to_string(repo.join(home)).unwrap();
    assert!(rule.contains("phase_write_allowed"), "the one writer no longer asks the phase rule");
}

/// A marca de aprovação saiu do código: nenhum arquivo de produção a grava
/// nem a lê, e nem a prosa do plugin nem o painel a ensinam. Cada arquivo
/// Rust é cortado no primeiro módulo de teste.
#[test]
fn the_approval_marker_is_gone_from_the_code() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in [
        "apps/rt/src",
        "apps/cli/src",
        "packages/core/src",
        "plugin",
        "apps/dashboard/src",
        "apps/dashboard/server/src",
    ] {
        text_files(&repo.join(dir), &mut files);
    }
    assert!(files.len() > 100, "the search reached the source tree: {} files", files.len());
    let mut hits = Vec::new();
    for (path, body) in &files {
        let production = body.split("#[cfg(test)]").next().unwrap_or_default();
        for needle in [".approved-by-user", "approval_marker_path", "APPROVED_BY_USER_MARKER"] {
            if production.contains(needle) {
                hits.push(format!("{} — {needle}", path.display()));
            }
        }
    }
    assert!(hits.is_empty(), "the approval marker is still in the code:\n{}", hits.join("\n"));
}
