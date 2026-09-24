// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A escolha do pedido só manda conferir as tarefas no código quando um
//! commit mudou arquivo delas depois do texto vigente: a tarefa escrita
//! depois do último commit não é nomeada, e a versão que a rodada grava só
//! para pôr a tarefa na onda não conta. Prova pelo binário de verdade, num
//! repositório temporário, nos dois idiomas.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

const SPEC: &str = "conferir";
const SESSION: &str = "s-conferir";
const GOAL: &str = "Somar dois números no programa.";

fn git(root: &Path, args: &[&str], date: Option<&str>) {
    let mut command = Command::new("git");
    command.args(args).current_dir(root);
    if let Some(date) = date {
        command.env("GIT_AUTHOR_DATE", date).env("GIT_COMMITTER_DATE", date);
    }
    let out = command.output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// O repositório de teste: `main` e `dev`, parado em `dev`, com o Mustard
/// fora do git, no idioma `language`, e um arquivo de código para a tarefa.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl Project {
    fn new(language: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("projeto");
        let home = dir.path().join("casa");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&home).expect("home");
        git(&root, &["init", "-q"], None);
        git(&root, &["config", "user.email", "t@example.com"], None);
        git(&root, &["config", "user.name", "t"], None);
        git(&root, &["config", "commit.gpgsign", "false"], None);
        git(&root, &["checkout", "-q", "-b", "main"], None);
        std::fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\ntarget/\n").expect("exclude");
        let config = json!({"language": {"text": language}, "git": {"flow": {"*": "dev", "dev": "main"}}});
        std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/main.rs"), "fn main() {\n    println!(\"{}\", 1 + 1);\n}\n").expect("code");
        git(&root, &["add", "-A"], None);
        git(&root, &["commit", "-q", "-m", "init"], None);
        git(&root, &["checkout", "-q", "-b", "dev"], None);
        Self { _dir: dir, root, home }
    }

    fn command(&self, args: &[&str], stdin: &str) -> std::process::Output {
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

    /// Um comando `run` que precisa responder `ok`.
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

    fn write(&self, event_type: &str, fields: &Value) -> Value {
        self.run(&["write", event_type, "--spec", SPEC, "--json", &fields.to_string()])
    }

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
    project
        .log()
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "message" && e.str_field("author") == Some("user") && e.str_field("text") == Some(text))
        .unwrap_or_else(|| panic!("the entry hook did not record the message"))
        .id
}

/// O levantamento inteiro, cada ponto fechado com uma decisão do projeto
/// todo: o item que faz a rodada pedir a escolha antes de soltar a onda.
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
        open["facts"] = json!([{"text": "A soma mora no programa.", "source": "src/main.rs:2"}]);
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

/// O plano de uma tarefa só, que muda `src/main.rs`, sem onda. Devolve o
/// número da tarefa.
fn plan(project: &Project) -> u64 {
    let said = user_says(project, "O plano é uma tarefa só, que soma dois números.");
    let criterion = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a soma aparece", "proof": "git --version", "form": "ubiquitous",
            "origin": said}),
    );
    let task = project.write(
        "task",
        &json!({"title": "Entregar a tarefa", "text": "Somar dois números no programa.", "files": [{"path": "src/main.rs"}],
            "depends_on": [], "covers": [criterion["id"]], "origin": said}),
    );
    project.run(&["plan", "--spec", SPEC]);
    task["id"].as_u64().expect("the task number")
}

/// O clique em "Aprovar" na pergunta da aprovação, pelo gancho da testemunha.
fn approve(project: &Project, lang: Locale) {
    let question = translate("approval.question", lang);
    let yes = translate("approval.option", lang);
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
}

/// O instante do evento `id`, em segundos.
fn seconds_of(log: &SpecLog, id: u64) -> i64 {
    let at = log.get(id).expect("the event").at().to_string();
    chrono::DateTime::parse_from_rfc3339(at.trim()).expect("a readable time").timestamp()
}

/// Numa spec aprovada, a tarefa escrita depois do último commit não é
/// nomeada e a escolha do pedido não manda conferir nada no código. Um
/// commit que toca o arquivo dela depois do texto — datado de um segundo
/// depois da tarefa, e ainda assim antes da versão que a rodada gravou para
/// pô-la na onda — faz a frase aparecer com o código dela: a versão que só
/// muda a onda não conta. Nos dois idiomas.
#[test]
fn a_conferencia_das_tarefas_so_e_pedida_quando_o_arquivo_mudou_depois_da_tarefa() {
    for (language, lang) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        let project = Project::new(language);
        project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
        survey(&project);
        let task = plan(&project);
        approve(&project, lang);
        let code = project.log().codes().get(&task).cloned().expect("the task code");
        let written = seconds_of(&project.log(), task);

        // A rodada só roda no segundo seguinte ao da tarefa: a versão que ela
        // grava para pôr a tarefa na onda fica com instante posterior ao texto.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while chrono::Utc::now().timestamp() <= written && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let check = translate("round.analysis_check", lang);
        let lead = check.split("{tasks}").next().expect("the text before the tasks").to_string();
        let asked = project.run(&["round", "--spec", SPEC]);
        assert_eq!(asked["analysis"][0]["wave"], json!(1), "{language}: {asked}");
        let next = asked["next"].as_str().unwrap_or_default();
        assert!(!next.contains(&lead), "{language}: a tarefa escrita depois do último commit pediu conferência: {next}");
        assert!(!next.contains(&code), "{language}: a tarefa escrita depois do último commit foi nomeada: {next}");

        // A rodada pôs a tarefa na onda numa versão nova, só com a onda.
        let log = project.log();
        let placed = log.visible().into_iter().find(|e| e.event_type == "task").expect("the task").clone();
        assert_eq!(placed.wave(), Some(1), "{language}: {placed:?}");
        assert_eq!(placed.replaced(), vec![task], "{language}: {placed:?}");
        assert_eq!(placed.str_field("text"), log.get(task).and_then(|e| e.str_field("text")), "{language}");
        assert!(seconds_of(&log, placed.id) > written, "{language}: a versão da onda saiu no mesmo segundo da tarefa");

        // O commit que toca o arquivo da tarefa, um segundo depois do texto.
        std::fs::write(project.root.join("src/main.rs"), "fn main() {\n    println!(\"{}\", 2 + 2);\n}\n").expect("the change");
        git(&project.root, &["add", "src/main.rs"], None);
        let date = format!("@{} +0000", written + 1);
        git(&project.root, &["commit", "-q", "-m", "a soma muda"], Some(&date));

        let again = project.run(&["round", "--spec", SPEC]);
        assert_eq!(again["analysis"][0]["wave"], json!(1), "{language}: {again}");
        let next = again["next"].as_str().unwrap_or_default();
        let named = check.replace("{tasks}", &code);
        assert!(next.contains(&named), "{language}: a frase não nomeou a tarefa com arquivo mudado: {next}");
    }
}
