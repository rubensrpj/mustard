// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A trava da aprovação vale no fluxo de hoje, de ponta a ponta.
//!
//! A spec que o `spec-draft` cria nasce na fase de plano, gravada no
//! `spec.ndjson`; o portão de escrita barra o código do projeto; "Ajustar" na
//! pergunta não muda nada; "Aprovar" grava a aprovação, e o código passa.
//!
//! O rascunho roda por `run_at`, com a pasta temporária como projeto; os
//! ganchos rodam como processo, porque são privados da biblioteca.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use mustard_rt::commands::spec::spec_draft::{run_at, SpecDraftOpts};
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

/// Roda o gancho `event` com `payload` e devolve a saída dele.
fn hook(root: &Path, event: &str, payload: Value) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["on", event])
        .current_dir(root)
        .env("CLAUDE_PROJECT_DIR", root)
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mustard-rt");
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
    );
    out.contains("\"deny\"")
}

/// Responde a pergunta de aprovação com `choice`.
fn answer(root: &Path, choice: &str) -> String {
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
    )
}

fn draft_opts() -> SpecDraftOpts {
    SpecDraftOpts {
        intent: "Cadastro de clientes".into(),
        slug: Some(SPEC.into()),
        scope: "light".into(),
        signals: None,
        output: None,
        material: None,
        material_only: false,
        no_material_reason: Some("fixture: a trava da aprovação é o que se prova aqui".into()),
        waves: 0,
        plan: None,
        force: false,
        query_terms: None,
        force_scope: false,
    }
}

#[test]
fn a_drafted_spec_blocks_code_until_the_user_approves() {
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

    assert_eq!(run_at(root, draft_opts()), 0);

    // O rascunho grava o nascimento pelo binário, e o `spec.md` dele fica.
    let spec_dir = root.join(".claude").join("spec").join(SPEC);
    let events = std::fs::read_to_string(spec_dir.join("spec.ndjson")).unwrap();
    let born: Value = serde_json::from_str(events.lines().next().unwrap()).unwrap();
    assert_eq!(born["type"], "state", "{born}");
    assert_eq!(born["phase"], "plan", "{born}");
    assert_eq!(born["branch"], format!("feature/{SPEC}"), "{born}");
    let md = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
    assert!(md.contains("Cadastro de clientes"), "the draft's document survives: {md}");

    // Antes da aprovação, o código do projeto é barrado.
    assert!(edit_is_blocked(root), "a drafted spec blocks code before the approval");

    // "Ajustar" não aprova: continua barrado.
    answer(root, "Ajustar");
    assert!(edit_is_blocked(root), "adjusting keeps the lock");

    // Com a narrativa ainda semeada, "Aprovar" não grava: as conferências do
    // `approve-spec` vêm antes, e o motivo vai ao assistente.
    let said = answer(root, "Aprovar");
    assert!(!said.contains("/clear"), "an unmet precondition records nothing: {said}");
    assert!(edit_is_blocked(root), "the lock stays while the spec cannot be approved");

    // A spec escrita, sem texto semeado e sem critério a provar.
    std::fs::write(spec_dir.join("spec.md"), "# Cadastro de clientes\n\n## Contexto\n\nClientes se cadastram sozinhos.\n")
        .unwrap();

    // "Aprovar" grava a aprovação, e o código passa.
    let said = answer(root, "Aprovar");
    assert!(said.contains("/clear"), "the witness suggests /clear: {said}");
    assert!(!edit_is_blocked(root), "after the approval the code is open");
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
