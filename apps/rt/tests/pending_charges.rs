//! Os fechamentos armados para a cobrança das pendências, pelo binário de
//! verdade: dois processos que fecham specs ao mesmo tempo armam os dois
//! fechamentos, porque o arquivo dos contadores é lido e gravado com a trava
//! do sistema presa.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

/// Uma spec em andamento, com o arquivo de eventos escrito à mão.
fn seed(root: &Path, spec: &str) {
    let dir = root.join(".claude").join("spec").join(spec);
    std::fs::create_dir_all(&dir).expect("spec folder");
    let state = json!({
        "v": 1, "id": 1, "type": "state", "phase": "running",
        "author": "binary", "at": "2026-09-13T10:00:00Z"
    });
    std::fs::write(dir.join("spec.ndjson"), format!("{state}\n")).expect("spec file");
}

/// O fechamento da spec `spec` pelo `emit-pipeline`, que passa pela ponte.
fn close(root: &Path, spec: &str) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "emit-pipeline", "--kind", "pipeline.complete", "--spec", spec, "--allow-no-qa"])
        .current_dir(root)
        // O `emit-pipeline` prefere o `CLAUDE_PROJECT_DIR` à pasta atual: sem
        // fixá-lo, o fechamento cairia no projeto de quem roda a suíte.
        .env("CLAUDE_PROJECT_DIR", root)
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn emit-pipeline")
}

#[test]
fn two_processes_closing_at_once_arm_both_closures() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mustard.json"), "{}").expect("config");
    let rounds = 10;
    let names: Vec<Vec<String>> =
        (0..rounds).map(|round| (0..2).map(|w| format!("spec-{round}-{w}")).collect()).collect();
    for spec in names.iter().flatten() {
        seed(root, spec);
    }

    for pair in &names {
        let closers: Vec<_> = pair.iter().map(|spec| close(root, spec)).collect();
        for closer in closers {
            let out = closer.wait_with_output().expect("wait emit-pipeline");
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        }
    }

    let raw = std::fs::read_to_string(root.join(".claude").join("pending").join("charges.json"))
        .expect("the charges file exists");
    let charges: Value = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("torn file ({e}): {raw}"));
    let mut armed: Vec<String> = charges["armed"]
        .as_array()
        .expect("an armed list")
        .iter()
        .filter_map(|charge| charge["spec"].as_str().map(str::to_string))
        .collect();
    armed.sort();
    let mut expected: Vec<String> = names.into_iter().flatten().collect();
    expected.sort();
    assert_eq!(armed, expected, "no closure armed at the same time was lost");
}
