// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! O fluxo inteiro de uma spec de teste, pelo binário de verdade, numa pasta
//! temporária: abrir, levantar, conferir o plano, aprovar pelo clique, duas
//! rodadas, fechar e abrir o pull request.
//!
//! A fala do usuário e o clique chegam pelos ganchos, como chegam numa sessão;
//! o agente da onda e o revisor são as linhas do fim que os textos deles
//! ensinam, e o provedor do pull request é um `gh` falso no começo do `PATH`.
//! No fim, cada passo do fluxo gravou uma chamada só — a aprovação nenhuma — e
//! a pasta da spec tem os três arquivos dela.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::State;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

const SPEC: &str = "ponta";
const GOAL: &str = "Trocar a saudação do programa.";
const SESSION: &str = "s-ponta";

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// O projeto de teste: um repositório com `main` e `dev`, parado em `dev`, com
/// as bases declaradas, o provedor do GitHub, o lint do projeto e o Mustard
/// fora do git; uma pasta pessoal falsa; e o `gh` falso, que anota cada
/// chamada e responde que a branch não tem pull request e que o criado é o 7.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    bin: PathBuf,
}

impl Project {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("projeto");
        let home = dir.path().join("casa");
        let bin = dir.path().join("bin");
        for folder in [&root, &home, &bin] {
            std::fs::create_dir_all(folder).expect("folder");
        }
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

        let gh = bin.join("gh");
        std::fs::write(
            &gh,
            "#!/bin/sh\necho \"$*\" >> \"$GH_LOG\"\ncase \"$1 $2\" in\n\
             \"pr view\") echo 'no pull requests found' >&2; exit 1 ;;\n\
             \"pr create\") echo 'https://github.com/exemplo/projeto/pull/7'; exit 0 ;;\n\
             esac\nexit 1\n",
        )
        .expect("the fake gh");
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        Self { _dir: dir, root, home, bin }
    }

    /// O binário com `args`, no projeto, com a pasta pessoal falsa, o `gh`
    /// falso à frente do `PATH` e nenhuma sessão nem spec forçada.
    fn command(&self, args: &[&str], stdin: &str) -> Output {
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap_or_default());
        let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(args)
            .current_dir(&self.root)
            .env("PATH", path)
            .env("GH_LOG", self.gh_log())
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
        let mut all = vec!["run"];
        all.extend_from_slice(args);
        let out = self.command(&all, "");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let report: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}{}", String::from_utf8_lossy(&out.stderr)));
        assert_eq!(report["ok"], json!(true), "{args:?}: {report}");
        report
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

    fn gh_log(&self) -> PathBuf {
        self.home.join("gh.log")
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
    assert_eq!(said.str_field("text"), Some(text));
    said.id
}

/// O levantamento inteiro: o objetivo, o `grill` e cada ponto gravado,
/// respondido e fechado.
fn survey(project: &Project) {
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
}

/// O plano de uma onda: o critério com a prova, a onda e a tarefa.
fn plan(project: &Project) {
    let said = user_says(project, "O plano é uma onda só, que muda a saudação.");
    let criterion = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "origin": said}),
    );
    project.write(
        "wave",
        &json!({"n": 1, "text": "Onda 1: a saudação nova.", "criteria": [criterion["id"]],
            "done_when": "A saudação nova aparece.", "origin": said}),
    );
    project.write(
        "task",
        &json!({"wave": 1, "text": "Trocar a saudação no programa.", "files": [{"path": "src/main.rs"}],
            "origin": said}),
    );
    let planned = project.run(&["plan", "--spec", SPEC]);
    assert_eq!(State::from_log(&project.log()).phase, Some("plan"), "{planned}");
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

/// Quantas chamadas cada comando do fluxo gravou na spec.
fn calls(project: &Project) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for call in project.log().visible().into_iter().filter(|e| e.event_type == "call") {
        *out.entry(call.str_field("command").unwrap_or_default().to_string()).or_insert(0) += 1;
    }
    out
}

/// Uma spec de teste roda de ponta a ponta com o binário novo: abrir,
/// levantar, conferir o plano, aprovar pelo clique, duas rodadas, fechar e
/// abrir o pull request funcionam em sequência, e a pasta da spec termina com
/// três arquivos. Cada passo do fluxo é uma chamada só: abrir 1, levantamento
/// 1 (as respostas gravadas não contam), plano 1, aprovar 0, cada rodada 1,
/// fechar 1 e pull request 1.
#[test]
fn a_test_spec_runs_end_to_end_one_call_per_step_and_leaves_three_files() {
    let project = Project::new();

    let opened = project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened["step"], json!("ask_goal"), "{opened}");
    survey(&project);
    plan(&project);
    approve(&project);

    // Primeira rodada: a onda sai numa cópia separada.
    let first = project.run(&["round", "--spec", SPEC]);
    let dispatched = first["dispatch"].as_array().cloned().unwrap_or_default();
    assert_eq!(dispatched.len(), 1, "{first}");
    let log = project.log();
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send").expect("the send");
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
    assert!(copy.join(".git").is_file(), "the wave works in a linked checkout");

    // O agente da onda muda o arquivo na cópia e devolve a linha do fim.
    std::fs::write(copy.join("src/main.rs"), "fn main() {\n    println!(\"olá\");\n}\n").expect("the change");
    let delivered = json!({"wave": 1, "text": "A saudação virou olá.", "files": ["src/main.rs"],
        "commit": "a saudação vira olá"});
    let second = project.run(&["round", "--spec", SPEC, "--report", &format!("<DELIVERED>{delivered}</DELIVERED>")]);
    let reviews = second["reviews"].as_array().cloned().unwrap_or_default();
    assert_eq!(reviews.len(), 1, "{second}");
    assert_eq!(std::fs::read_to_string(project.root.join("src/main.rs")).unwrap(), "fn main() {\n    println!(\"olá\");\n}\n");

    // O revisor aprova, e o fechamento grava o veredito, roda o lint e o
    // critério e fecha, sem revisão final numa spec de uma onda.
    let verdict = json!({"wave": 1, "result": "approved", "text": "A saudação mudou.",
        "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}]});
    let closed = project.run(&["close", "--spec", SPEC, "--report", &format!("<VERDICT>{verdict}</VERDICT>")]);
    assert_eq!(closed["phase"], json!("closed"), "{closed}");
    assert!(closed.get("review").is_none(), "{closed}");
    let pr_line = format!("mustard-rt run pr-open --base dev --head feature/{SPEC} --spec {SPEC}");
    assert_eq!(closed["command"], json!(pr_line), "{closed}");

    // O pull request abre pela linha que o fechamento devolveu.
    let argv: Vec<&str> = pr_line.split_whitespace().skip(2).collect();
    let pr = project.run(&argv);
    assert_eq!(pr["number"], json!(7), "{pr}");
    let asked = std::fs::read_to_string(project.gh_log()).expect("the fake gh was called");
    assert!(asked.lines().any(|l| l.starts_with("pr create") && l.contains("--head feature/ponta")), "{asked}");
    let state = State::from_log(&project.log());
    assert_eq!(state.phase, Some("pr_open"), "{pr}");

    let expected: BTreeMap<String, usize> =
        [("open", 1), ("grill", 1), ("plan", 1), ("round", 2), ("close", 1), ("pr-open", 1)]
            .into_iter()
            .map(|(command, count)| (command.to_string(), count))
            .collect();
    assert_eq!(calls(&project), expected, "each flow step is one call, and the approval none");

    let folder = project.root.join(".claude/spec").join(SPEC);
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .expect("the spec folder")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    assert_eq!(names, ["spec.html", "spec.md", "spec.ndjson"], "the spec folder ends with three files");
}
