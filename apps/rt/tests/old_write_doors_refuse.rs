// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Os comandos antigos que gravavam na spec — o QA, os fechamentos, o
//! veredito, os critérios e o merge — recusam na entrada, nos dois idiomas,
//! sem gravar nada: o arquivo de eventos da spec e a lista de pendências ficam
//! com os mesmos bytes, e nenhuma cobrança é armada.
//!
//! Tudo roda pelo binário, com o `CLAUDE_PROJECT_DIR` na pasta temporária e
//! sem as variáveis de sessão, para nada cair no projeto de verdade.

use std::path::Path;
use std::process::{Command, Output};

use mustard_core::platform::i18n::{translate, Locale};
use serde_json::Value;

const PT: &str = r#"{"language":{"text":"pt-BR"},"git":{"flow":{"*":"dev","dev":"main"}}}"#;
const EN: &str = r#"{"language":{"text":"en-US"},"git":{"flow":{"*":"dev","dev":"main"}}}"#;

/// A spec aberta em cada projeto destes testes.
const SPEC: &str = "cadastro";

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `mustard-rt run <args>` no projeto `root`.
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .arg("run")
        .args(args)
        .current_dir(root)
        .env("CLAUDE_PROJECT_DIR", root)
        .env_remove("MUSTARD_PROJECT_ROOT")
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .env_remove("MUSTARD_APPROVAL_MODE")
        .output()
        .expect("run mustard-rt")
}

/// Um repositório com `dev` e `main`, uma pendência na lista e a spec
/// `cadastro` aberta a partir dela pelo `open`: a pendência leva a nota de
/// que virou esta spec, que é o que o merge lê.
fn project(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["checkout", "-q", "-b", "main"]);
    std::fs::write(root.join(".git").join("info").join("exclude"), ".claude/\nmustard.json\n").unwrap();
    std::fs::write(root.join("mustard.json"), config).unwrap();
    std::fs::write(root.join("README.md"), "oi\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "init"]);
    git(root, &["checkout", "-q", "-b", "dev"]);

    let added = run(root, &["pending", "--add", "--title", "Humanize", "--detail", "nasceu na conversa"]);
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stdout));
    let opened = run(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev", "--pending", "P-1"]);
    assert!(opened.status.success(), "{}", String::from_utf8_lossy(&opened.stdout));
    assert!(spec_file(root).is_file(), "the spec was opened");
    dir
}

fn spec_file(root: &Path) -> std::path::PathBuf {
    root.join(".claude").join("spec").join(SPEC).join("spec.ndjson")
}

fn ledger(root: &Path) -> std::path::PathBuf {
    root.join(".claude").join("pending").join("ledger.json")
}

fn charges(root: &Path) -> std::path::PathBuf {
    root.join(".claude").join("pending").join("charges.json")
}

/// A recusa impressa: exit 1, a razão curta e a mensagem.
fn refusal(out: &Output) -> Value {
    assert_eq!(out.status.code(), Some(1), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).expect("a JSON refusal")
}

/// Cada porta antiga: os argumentos, a razão, a chave da mensagem e o nome do
/// comando que a mensagem traz.
fn doors() -> Vec<(Vec<&'static str>, &'static str, &'static str, &'static str)> {
    vec![
        (vec!["qa-run", "--spec", SPEC], "wait-for-close", "retired.wait_close", "qa-run"),
        (vec!["close-pipeline", "--spec", SPEC], "wait-for-close", "retired.wait_close", "close-pipeline"),
        (vec!["close-orchestrate", "--spec", SPEC], "wait-for-close", "retired.wait_close", "close-orchestrate"),
        (vec!["complete-spec", SPEC], "wait-for-close", "retired.wait_close", "complete-spec"),
        (vec!["complete-spec", SPEC, "--archive"], "wait-for-close", "retired.wait_close", "complete-spec"),
        (
            vec!["review-result", "--spec", SPEC, "--verdict", "approved"],
            "wait-for-round",
            "retired.wait_round",
            "review-result",
        ),
        (vec!["pr-review", "--verdict", "approved"], "wait-for-round", "retired.wait_round", "pr-review --verdict"),
        (vec!["pr-merge"], "wait-for-merge", "retired.wait_merge", ""),
        (
            vec![
                "ac-add", "--spec", SPEC, "--ac", "AC-9", "--statement", "quando roda, então passa", "--command",
                "exit 0", "--reason", "critério novo",
            ],
            "use-write-criterion",
            "retired.use_write_criterion",
            "ac-add",
        ),
        (
            vec!["ac-amend", "--spec", SPEC, "--ac", "AC-1", "--command", "exit 0", "--reason", "comando trocado"],
            "use-write-criterion",
            "retired.use_write_criterion",
            "ac-amend",
        ),
    ]
}

/// Cada comando antigo que gravava na spec recusa, nos dois idiomas, e nada
/// muda: nem o arquivo de eventos, nem a lista de pendências, e nenhuma
/// cobrança nasce.
#[test]
fn every_retired_write_door_refuses_and_leaves_the_spec_file_untouched() {
    for (config, lang) in [(PT, Locale::PtBr), (EN, Locale::EnUs)] {
        let dir = project(config);
        let root = dir.path();
        let events_before = std::fs::read(spec_file(root)).unwrap();
        let ledger_before = std::fs::read(ledger(root)).unwrap();

        for (args, reason, key, command) in doors() {
            let out = run(root, &args);
            let report = refusal(&out);
            assert_eq!(report["reason"], reason, "{args:?}: {report}");
            let expected = translate(key, lang).replace("{command}", command);
            assert_eq!(report["hint"], expected, "{args:?}: {report}");
            assert_eq!(std::fs::read(spec_file(root)).unwrap(), events_before, "{args:?} wrote to the spec");
            assert_eq!(std::fs::read(ledger(root)).unwrap(), ledger_before, "{args:?} touched the list");
            assert!(!charges(root).exists(), "{args:?} armed a charge");
        }
    }
}

/// Nenhum comando antigo arma a cobrança das pendências, por caminho nenhum:
/// nem os que recusam (a aprovação entre eles), nem os que ainda passam pela
/// porta de dentro (o fim de onda e os tipos que só gravam no log velho). Entre esta
/// rodada e os comandos novos, ninguém fecha nem entrega pelo Mustard, e o
/// arquivo dos contadores nem nasce.
#[test]
fn no_old_command_arms_the_pending_charge() {
    let dir = project(PT);
    let root = dir.path();
    // A spec em execução, com um pedido adiado que aponta a pendência: há o
    // que cobrar, se alguém fechasse.
    let said = run(root, &["write", "message", "--spec", SPEC, "--json", r#"{"author":"user","text":"o Humanize fica"}"#]);
    assert!(said.status.success(), "{}", String::from_utf8_lossy(&said.stdout));
    let origin: Value = serde_json::from_slice(&said.stdout).unwrap();
    let deferred = format!(
        r#"{{"text":"o Humanize fica para depois","keys":["humanize"],"pending":1,"origin":{}}}"#,
        origin["id"]
    );
    let adiado = run(root, &["write", "deferred", "--spec", SPEC, "--json", &deferred]);
    assert!(adiado.status.success(), "{}", String::from_utf8_lossy(&adiado.stdout));
    let running = r#"{"v":1,"id":900,"at":"2026-09-14T10:00:00-03:00","type":"state","author":"binary","phase":"running"}"#;
    let mut events = std::fs::read_to_string(spec_file(root)).unwrap();
    events.push_str(running);
    events.push('\n');
    std::fs::write(spec_file(root), &events).unwrap();
    // Com o `meta.json` ao lado, as portas de dentro fazem o serviço inteiro:
    // é por ele que o fim da última onda fechava a spec.
    std::fs::write(
        spec_file(root).with_file_name("meta.json"),
        r#"{"scope":"light","stage":"Execute","outcome":"Active","phase":"EXECUTE","totalWaves":1}"#,
    )
    .unwrap();

    let mut doors: Vec<Vec<&str>> = doors().into_iter().map(|(args, ..)| args).collect();
    // A aprovação recusa na entrada; o fim de onda move o estágio por dentro,
    // e o tipo de fase só grava no log velho.
    doors.push(vec!["approve-spec", "--spec", SPEC]);
    doors.push(vec!["wave-done", "--spec", SPEC, "--wave", "1"]);
    doors.push(vec!["emit-pipeline", "--kind", "pipeline.phase", "--spec", SPEC, "--payload", "{}"]);
    doors.push(vec!["emit-pipeline", "--kind", "pipeline.complete", "--spec", SPEC, "--payload", "{}"]);
    for args in doors {
        run(root, &args);
        assert!(!charges(root).exists(), "{args:?} armed a charge");
        let now = std::fs::read_to_string(spec_file(root)).unwrap();
        assert!(!now.contains("\"closed\"") && !now.contains("\"delivered\""), "{args:?} moved the phase: {now}");
    }
}

/// O merge recusa antes de qualquer coisa: a pendência que virou a spec
/// continua aberta, e a spec não fica entregue.
#[test]
fn a_merge_is_refused_and_the_linked_pending_stays_open() {
    let dir = project(PT);
    let root = dir.path();

    let out = run(root, &["pr-merge"]);
    assert_eq!(refusal(&out)["reason"], "wait-for-merge");

    let list: Value = serde_json::from_slice(&std::fs::read(ledger(root)).unwrap()).unwrap();
    let item = &list["items"][0];
    assert_eq!(item["id"], "P-1", "{list}");
    assert_eq!(item["status"], "open", "the item that became the spec stays open: {list}");
    let events = std::fs::read_to_string(spec_file(root)).unwrap();
    assert!(!events.contains("delivered"), "nothing was delivered: {events}");
}
