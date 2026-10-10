// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Publicação externa é explícita: aprovação, rodada e mudanças do pedido
//! preservam apenas o estado local. O comando de publicação prepara uma
//! versão sanitizada e imutável, sem alegar transporte remoto.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

const SPEC: &str = "plano";
const SESSION: &str = "s-plano";
const GOAL: &str = "Somar dois números no programa.";

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// O repositório de teste: `main` e `dev`, parado em `dev`, com o Mustard
/// fora do git e um arquivo de código para as tarefas mexerem.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    lang: Locale,
}

impl Project {
    fn new() -> Self {
        Self::in_language(Locale::PtBr)
    }

    /// O repositório de teste com o Mustard falando `lang`.
    fn in_language(lang: Locale) -> Self {
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
        let config = json!({"language": {"text": lang.as_str()}, "git": {"flow": {"*": "dev", "dev": "main"}}});
        std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("code");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        git(&root, &["checkout", "-q", "-b", "dev"]);
        Self { _dir: dir, root, home, lang }
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
            .env_remove("TYPESAFE_API_KEY")
            .env_remove("MUSTARD_JEV_URL")
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

    fn run(&self, args: &[&str]) -> Value {
        let report = self.answer(args);
        assert_eq!(report["ok"], json!(true), "{args:?}: {report}");
        report
    }

    fn answer(&self, args: &[&str]) -> Value {
        let mut all = vec!["run"];
        all.extend_from_slice(args);
        let out = self.command(&all, "");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}{}", String::from_utf8_lossy(&out.stderr)))
    }

    fn write(&self, event_type: &str, fields: &Value) -> Value {
        self.run(&["write", event_type, "--spec", SPEC, "--json", &fields.to_string()])
    }

    /// A gravação com `--copy`, a última de um pedido.
    fn write_copying(&self, event_type: &str, fields: &Value) -> Value {
        self.run(&["write", event_type, "--spec", SPEC, "--json", &fields.to_string(), "--copy"])
    }

    fn hook(&self, event: &str, payload: &Value) {
        let out = self.command(&["on", event], &payload.to_string());
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
    }
}

/// A fala do usuário, pelo gancho da entrada; devolve o número dela.
fn user_says(project: &Project, text: &str) -> u64 {
    project.hook(
        "UserPromptSubmit",
        &json!({"hook_event_name": "UserPromptSubmit", "prompt": text, "session_id": SESSION,
            "cwd": project.root.to_string_lossy()}),
    );
    let events =
        std::fs::read_to_string(project.root.join(".claude/spec").join(SPEC).join("spec.ndjson")).unwrap_or_default();
    events
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|e| e["type"] == json!("message") && e["author"] == json!("user") && e["text"] == json!(text))
        .filter_map(|e| e["id"].as_u64())
        .next_back()
        .unwrap_or_else(|| panic!("the entry hook did not record the message"))
}

/// O levantamento inteiro: o objetivo, o `grill`, que grava os pontos, e
/// cada ponto com os fatos somados, respondido e fechado, exatamente como uma
/// sessão de verdade faz.
fn survey(project: &Project) {
    let said = user_says(project, GOAL);
    project.write("context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));
    let grilled = project.run(&["grill", "--spec", SPEC, "--kinds", "feature"]);
    let points = grilled["points"].as_array().cloned().expect("the point list");
    assert!(!points.is_empty(), "{grilled}");
    let mut current = Value::Null;
    for point in &points {
        let facts = json!([{"text": "A soma mora no programa.", "source": "src/main.rs:1"}]);
        current = project.write("point", &json!({"replaces": point["id"], "facts": facts}))["point"].clone();
    }
    for point in &points {
        let code = current["code"].as_str().expect("the open point").to_string();
        let answer = project.write(
            "decision",
            &json!({"title": "Combinar o item", "agent": format!("- ponto {code}"), "text": "O usuário respondeu ao ponto.", "keys": ["levantamento"],
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

/// O plano de uma tarefa só: o critério com a prova e a tarefa que o cobre,
/// sem onda — a onda nasce da rodada, pelo backlog; devolve o número da
/// mensagem que o origina, o do critério e a resposta do plano, que é a da
/// aprovação.
fn plan(project: &Project) -> (u64, u64, Value) {
    let said = user_says(project, "O plano é uma tarefa só, que soma dois números.");
    let criterion = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a soma aparece", "proof": "git --version", "form": "ubiquitous",
            "origin": said}),
    )["id"]
        .as_u64()
        .expect("the criterion id");
    project.write(
        "task",
        &json!({"agent": "- conferir pelo teste", "title": "Entregar a tarefa", "text": "Somar dois números no programa.", "files": [{"path": "src/main.rs"}],
            "depends_on": [], "covers": [criterion], "origin": said}),
    );
    let report = project.run(&["plan", "--spec", SPEC]);
    (said, criterion, report)
}

/// O clique em "Aprovar" na pergunta da aprovação, pelo gancho da testemunha.
fn approve(project: &Project) {
    let question = translate("approval.question", project.lang);
    let yes = translate("approval.option", project.lang);
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


fn no_automatic_publication(project: &Project, report: &Value) {
    assert!(report.get("publish").is_none(), "{report}");
    assert!(report.get("copy").is_none(), "{report}");
    assert!(!project.root.join(".claude/spec").join(SPEC).join("copy").exists());
}

#[test]
fn approval_round_and_request_do_not_export_without_an_explicit_publication() {
    let project = Project::new();
    let opened = project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened["step"], json!("ask_goal"));
    survey(&project);
    let (_, _, planned) = plan(&project);
    no_automatic_publication(&project, &planned);
    approve(&project);
    let round = project.run(&["round", "--spec", SPEC]);
    no_automatic_publication(&project, &round);
    assert!(!project.root.join(".claude/mustard/publications").exists());

    let exported = project.run(&["publish", "--spec", SPEC]);
    assert_eq!(exported["published"], false);
    assert_eq!(exported["prepared"], true);
    let database = Path::new(exported["database"].as_str().unwrap());
    let before = std::fs::read(database).unwrap();
    let public: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(public["spec"], SPEC);
    assert!(public.get("consumption").is_none());
    assert!(Path::new(exported["page"].as_str().unwrap()).is_file());

    let asked = user_says(&project, "Incluir também a subtração.");
    let request = project.write("request", &json!({"title":"Combinar o item", "text":"Incluir a subtração.",
        "keys":["subtração"], "effect":"new_waves", "origin":asked}));
    assert!(!request["next"].as_str().unwrap_or_default().contains("--copy"));
    let criterion = project.write("criterion", &json!({"title":"Combinar o item", "when":"o programa roda", "then":"a subtração aparece",
        "proof":"git --version", "form":"ubiquitous", "origin":asked}));
    let task = project.write_copying("task", &json!({"agent":"- conferir pelo teste", "title":"Entregar a subtração", "text":"Subtrair dois números no programa.",
        "files":[{"path":"src/main.rs"}], "depends_on":[], "covers":[criterion["id"]], "origin":asked}));
    for report in [&request, &criterion, &task] {
        no_automatic_publication(&project, report);
    }
    assert_eq!(std::fs::read(database).unwrap(), before, "ordinary writes never update the exported snapshot");
}

#[test]
fn an_explicit_export_reuses_unchanged_public_state_and_versions_a_phase_change() {
    for lang in [Locale::PtBr, Locale::EnUs] {
        let project = Project::in_language(lang);
        project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
        survey(&project);
        let (said, _, planned) = plan(&project);
        no_automatic_publication(&project, &planned);
        let first = project.run(&["publish", "--spec", SPEC]);
        let database = Path::new(first["database"].as_str().unwrap());
        let before = std::fs::read(database).unwrap();
        let note = project.write_copying("note", &json!({"title":"Combinar o item", "text":"Nota privada de 10/2026 que não será publicada.", "keys":["k"], "origin":said}));
        no_automatic_publication(&project, &note);
        let second = project.run(&["publish", "--spec", SPEC]);
        assert_eq!(second["snapshot_id"], first["snapshot_id"], "private prose is outside the external snapshot");
        assert_eq!(std::fs::read(database).unwrap(), before);
        approve(&project);
        let third = project.run(&["publish", "--spec", SPEC]);
        assert_ne!(third["snapshot_id"], first["snapshot_id"], "approval changes the public phase");
        assert_eq!(std::fs::read(database).unwrap(), before, "the previous version stays immutable");
        assert_eq!(third["published"], false);
        let body = std::fs::read_to_string(third["database"].as_str().unwrap()).unwrap();
        assert!(!body.contains("Nota privada") && !body.contains("src/main.rs") && !body.contains("casa"));
    }
}
