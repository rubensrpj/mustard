// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A cesta e o teto do pedido, ligados à rodada de verdade, pelo binário numa
//! pasta temporária.
//!
//! Uma spec aprovada com tarefas soltas na cesta, sem onda gravada nenhuma,
//! tem a rodada formando o lote sozinha, com o evento de onda de autor
//! binário — antes disso, quem decidia as ondas prontas só lia onda já
//! gravada, e a cesta ficava presa, formada só por teste.
//!
//! Uma spec aprovada com uma tarefa cujo texto passa do teto de tokens do
//! pedido tem a rodada recusada, com o tamanho medido e o teto, sem gravar
//! envio nenhum.

#![cfg(unix)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::State;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

const SPEC: &str = "cesta";
const GOAL: &str = "Trocar a saudação do programa.";
const SESSION: &str = "s-cesta";

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Um projeto de teste, com `dev` a partir de `main`, pronto para abrir uma
/// spec pelo binário de verdade.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl Project {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("projeto");
        let home = dir.path().join("casa");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&home).expect("home");
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "t@example.com"]);
        git(&root, &["config", "user.name", "t"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        git(&root, &["checkout", "-q", "-b", "main"]);
        std::fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\ntarget/\n").expect("exclude");
        let config = json!({
            "language": {"text": "pt-BR"},
            "git": {"flow": {"*": "dev", "dev": "main"}, "provider": "github"},
            "lintCommand": "git --version",
        });
        std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/main.rs"), "fn main() {\n    println!(\"oi\");\n}\n").expect("code");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        git(&root, &["checkout", "-q", "-b", "dev"]);
        Self { _dir: dir, root, home }
    }

    fn command(&self, args: &[&str], stdin: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CLAUDE_PROJECT_DIR", &self.root)
            .env("MUSTARD_CLAUDE_BIN", self.home.join("sem-claude"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CLAUDE_PLUGIN_ROOT")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .env_remove("MUSTARD_SESSION_ID")
            .env_remove("CLAUDE_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(stdin.as_bytes());
        }
        child.wait_with_output().expect("the binary finishes")
    }

    /// Um comando `run`, que precisa responder `ok`.
    fn run(&self, args: &[&str]) -> Value {
        let report = self.answer(args);
        assert_eq!(report["ok"], json!(true), "{args:?}: {report}");
        report
    }

    /// Um comando `run` que pode recusar: a resposta vem como veio.
    fn answer(&self, args: &[&str]) -> Value {
        let mut all = vec!["run"];
        all.extend_from_slice(args);
        let out = self.command(&all, "");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}{}", String::from_utf8_lossy(&out.stderr)))
    }

    /// Uma gravação pelo `run write`.
    fn write(&self, event_type: &str, fields: &Value) -> Value {
        self.run(&["write", event_type, "--spec", SPEC, "--json", &fields.to_string()])
    }

    /// Um evento do harness entregue ao gancho, como a sessão entrega.
    fn hook(&self, event: &str, payload: &Value) {
        let out = self.command(&["on", event], &payload.to_string());
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn log(&self) -> SpecLog {
        store::read(&store::spec_file(&self.root, SPEC).expect("spec file")).expect("readable").expect("the spec file")
    }
}

/// A fala do usuário, pelo gancho da entrada; devolve o número dela.
fn user_says(project: &Project, text: &str) -> u64 {
    project.hook(
        "UserPromptSubmit",
        &json!({"hook_event_name": "UserPromptSubmit", "prompt": text, "session_id": SESSION,
            "cwd": project.root.to_string_lossy()}),
    );
    let log = project.log();
    let said = log
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "message" && e.str_field("author") == Some("user"))
        .expect("the entry hook recorded the message");
    said.id
}

/// O levantamento inteiro: o objetivo, o `grill` e cada ponto gravado,
/// respondido e fechado.
fn survey(project: &Project) -> u64 {
    let said = user_says(project, GOAL);
    project.write("context", &json!({"text": GOAL, "origin": said}));
    let grilled = project.run(&["grill", "--spec", SPEC, "--kinds", "feature"]);
    let points = grilled["points"].as_array().cloned().expect("the point list");
    assert!(!points.is_empty(), "{grilled}");
    let mut current = Value::Null;
    for point in &points {
        let mut open = point.clone();
        open["status"] = json!("open");
        open["facts"] = json!([{"text": "A saudação mora no programa.", "source": "src/main.rs:2"}]);
        current = project.write("point", &open)["point"].clone();
    }
    for point in &points {
        let code = current["code"].as_str().expect("the open point").to_string();
        let answer = project.write(
            "decision",
            &json!({"text": format!("Resposta ao ponto {code}."), "keys": ["levantamento"],
                "why": "o usuário respondeu", "origin": said, "applies_to": {"files": ["**"]}}),
        );
        let closed = project.write(
            "point",
            &json!({"block": point["block"], "gap": point["gap"], "from": "gap", "status": "closed",
                "closes": code, "result": [answer["id"]], "origin": said}),
        );
        current = closed["point"].clone();
    }
    said
}

/// O clique em "Aprovar" na pergunta da aprovação, pelo gancho da testemunha.
fn approve(project: &Project) {
    let question = translate("approval.question", Locale::PtBr);
    let yes = translate("approval.option", Locale::PtBr);
    project.hook(
        "PostToolUse",
        &json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "AskUserQuestion",
            "tool_input": {"questions": [{"question": question, "options": [{"label": yes}, {"label": "Ajustar"}]}]},
            "tool_response": {"answers": {question: yes}},
            "session_id": SESSION,
            "cwd": project.root.to_string_lossy(),
        }),
    );
    assert_eq!(State::from_log(&project.log()).phase, Some("approved"));
}

/// Uma spec aprovada sem onda gravada, só com tarefas soltas na cesta: a
/// rodada, antes de escolher as ondas prontas, forma o lote sozinha e grava
/// o evento de onda com autor binário — sem esse fio, quem decide as ondas
/// prontas só lê onda já gravada, e a cesta nunca sai do lugar.
#[test]
fn a_round_forms_a_lot_from_the_basket_and_records_it_as_the_binarys_wave() {
    let project = Project::new();
    project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    let said = survey(&project);
    let criterion = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    project.write(
        "task",
        &json!({"text": "Trocar a saudação no programa.", "files": [{"path": "src/main.rs"}],
            "depends_on": [], "covers": [criterion["id"]], "origin": said}),
    );
    project.run(&["plan", "--spec", SPEC]);
    approve(&project);

    let before = project.log();
    assert!(before.visible().into_iter().all(|e| e.event_type != "wave"), "no hand-made wave before the round");

    project.run(&["round", "--spec", SPEC]);

    let after = project.log();
    let wave = after.visible().into_iter().find(|e| e.event_type == "wave").expect("the round forms a lot from the basket");
    assert_eq!(wave.str_field("author"), Some("binary"), "the lot the round forms is the binary's, not hand-designed");
}

/// Uma spec aprovada com uma tarefa cujo pedido passa do teto de tokens: a
/// rodada forma o lote pela cesta, mede o pedido antes de gravar o envio e
/// recusa, dizendo o tamanho medido e o teto de 25 mil, sem gravar nada.
#[test]
fn a_round_refuses_a_wave_whose_request_passes_the_token_cap() {
    let project = Project::new();
    project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    let said = survey(&project);
    // O pronto-quando da onda abre o pedido de verdade, e a onda que a cesta
    // forma o tira da prova do critério que a tarefa cobre: é a prova que
    // precisa ser grande, bem acima do teto de 25 mil tokens (perto de quatro
    // caracteres por token) — cento e vinte mil caracteres passam do teto.
    let huge = "a".repeat(120_000);
    let criterion = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a saudação nova aparece",
            "proof": format!("git --version {huge}"), "form": "ubiquitous", "origin": said}),
    );
    project.write(
        "task",
        &json!({"text": "Trocar a saudação no programa.", "files": [{"path": "src/main.rs"}],
            "depends_on": [], "covers": [criterion["id"]], "origin": said}),
    );
    project.run(&["plan", "--spec", SPEC]);
    approve(&project);

    let asked = project.run(&["round", "--spec", SPEC]);
    assert_eq!(asked["dispatch"], json!([]), "{asked}");
    let answer = json!({"wave": 1, "removed": [], "added": []});
    let refused = project.answer(&["round", "--spec", SPEC, "--report", &format!("<ANALYSIS>{answer}</ANALYSIS>")]);

    assert_eq!(refused["ok"], json!(false), "{refused}");
    assert_eq!(refused["reason"], json!("wave-token-cap"), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    assert!(hint.contains("onda 1 tem"), "the message names the wave: {refused}");
    assert!(hint.contains("acima do teto de 25000"), "the message names the measured size and the cap: {refused}");

    let log = project.log();
    assert!(log.visible().into_iter().all(|e| e.event_type != "send"), "the round refuses before recording the send");
}
