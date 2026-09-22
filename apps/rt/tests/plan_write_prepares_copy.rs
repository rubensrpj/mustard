// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Toda gravação que muda o plano — uma onda, uma tarefa, um critério ou um
//! item do combinado — de uma spec já aprovada prepara a cópia da página do
//! mesmo jeito que um pedido do usuário faz hoje, mesmo sem pedido nenhum no
//! mesmo passo: prova de ponta a ponta, pelo binário de verdade, num
//! repositório temporário. Antes desta obra só o pedido preparava, e ele
//! chegava antes das ondas novas, então a cópia saía sem elas.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const SPEC: &str = "plano";
const SESSION: &str = "s-plano";
const GOAL: &str = "Somar dois números no programa.";
const SPEC_URL: &str = "https://claude.ai/code/artifact/plano";

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
        let config = json!({"language": {"text": "pt-BR"}, "git": {"flow": {"*": "dev", "dev": "main"}}});
        std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("code");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        git(&root, &["checkout", "-q", "-b", "dev"]);
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

/// O levantamento inteiro: o objetivo, o `grill` e cada ponto gravado,
/// respondido e fechado, exatamente como uma sessão de verdade faz.
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
        open["facts"] = json!([{"text": "A soma mora no programa.", "source": "src/main.rs:1"}]);
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

/// O plano de uma onda: o critério com a prova, a onda e a tarefa; devolve o
/// número da mensagem que o origina e o do critério.
fn plan(project: &Project) -> (u64, u64) {
    let said = user_says(project, "O plano é uma onda só, que soma dois números.");
    let criterion = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a soma aparece", "proof": "git --version", "form": "ubiquitous",
            "origin": said}),
    )["id"]
        .as_u64()
        .expect("the criterion id");
    project.write(
        "wave",
        &json!({"n": 1, "text": "Onda 1: a soma.", "criteria": [criterion], "done_when": "A soma aparece.",
            "origin": said}),
    );
    project.write(
        "task",
        &json!({"wave": 1, "text": "Somar dois números no programa.", "files": [{"path": "src/main.rs"}],
            "depends_on": [], "origin": said}),
    );
    project.run(&["plan", "--spec", SPEC]);
    (said, criterion)
}

/// O clique em "Aprovar" na pergunta da aprovação, pelo gancho da testemunha.
fn approve(project: &Project) {
    let question = mustard_core::platform::i18n::translate("approval.question", mustard_core::platform::i18n::Locale::PtBr);
    let yes = mustard_core::platform::i18n::translate("approval.option", mustard_core::platform::i18n::Locale::PtBr);
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

/// O que a conversa faz com a ordem de um marco: publica cada página que
/// ainda não tem endereço e grava o endereço, e grava a cópia feita de cada
/// página com o `record` que a resposta trouxe.
fn follow(project: &Project, report: &Value) {
    for page in report["publish"].as_array().cloned().unwrap_or_default() {
        let url = if page == json!("spec") { SPEC_URL } else { "https://claude.ai/code/artifact/projeto" };
        project.write("publish", &json!({"page": page, "milestone": "round", "ok": true, "template": true, "url": url}));
    }
    for page in ["spec", "project"] {
        if !report["copy"][page].is_null() {
            project.write("copy", &report["copy"][page]["record"]);
        }
    }
}

/// Cada número de item que os lotes da cópia da spec, no disco, levam: lê
/// todo arquivo `spec-*.json` da pasta de cópia — cada escrita aponta, em
/// `file_path`, o arquivo com o corpo de verdade — e junta os `id` de
/// `items` de toda escrita da coleção `ranges`.
fn copied_items(project: &Project) -> Vec<u64> {
    let folder = project.root.join(".claude/spec").join(SPEC).join("copy");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&folder) else { return out };
    let mut batches: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("spec-") && n.ends_with(".json")))
        .collect();
    batches.sort();
    for path in batches {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(writes) = serde_json::from_str::<Vec<Value>>(&text) else { continue };
        for write in &writes {
            if write["collection"] != json!(mustard_core::platform::page_templates::RANGES) {
                continue;
            }
            let Some(file) = write["file_path"].as_str() else { continue };
            let Ok(body_text) = std::fs::read_to_string(project.root.join(file)) else { continue };
            let Ok(body) = serde_json::from_str::<Value>(&body_text) else { continue };
            for item in body["items"].as_array().into_iter().flatten() {
                if let Some(id) = item["id"].as_u64() {
                    out.push(id);
                }
            }
        }
    }
    out
}

/// A onda, a tarefa, o critério ou o item do combinado gravado numa spec já
/// aprovada, sem pedido do usuário no mesmo passo, traz na própria resposta a
/// cópia preparada para a página, com esse item dentro, e manda copiá-la —
/// antes desta obra só o pedido do usuário preparava a cópia, e ele vinha
/// antes das ondas novas: a cópia saía sem elas, e a página ficava sem a onda
/// nova até a rodada seguinte.
#[test]
fn gravacao_no_plano_depois_da_aprovacao_prepara_a_copia_da_pagina() {
    let project = Project::new();
    let opened = project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened["step"], json!("ask_goal"), "{opened}");
    survey(&project);
    let (said, _criterion) = plan(&project);
    approve(&project);

    // A primeira rodada depois da aprovação prepara o primeiro marco: a
    // página da spec ainda não tem endereço, e a resposta manda publicá-la e
    // copiar os lotes; a conversa segue a ordem, como faria de verdade.
    let first_round = project.run(&["round", "--spec", SPEC]);
    assert!(first_round["publish"].as_array().is_some_and(|p| p.iter().any(|p| p == "spec")), "{first_round}");
    follow(&project, &first_round);
    let before = copied_items(&project);

    // Uma onda nova, gravada direto — sem pedido do usuário no mesmo passo —,
    // numa spec já aprovada e já publicada: a resposta já traz a cópia
    // preparada com essa onda dentro, e manda copiá-la.
    let said2 = user_says(&project, "Incluir também a subtração.");
    let criterion2 = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a subtração aparece", "proof": "git --version", "form": "ubiquitous",
            "origin": said2}),
    );
    let wave2 = project.write(
        "wave",
        &json!({"n": 2, "text": "Onda 2: a subtração.", "criteria": [criterion2["id"]],
            "done_when": "A subtração aparece.", "origin": said2}),
    );
    assert!(wave2["copy"].is_object(), "a gravação do plano não trouxe a cópia preparada: {wave2}");
    let next = wave2["next"].as_str().unwrap_or_default();
    assert!(next.contains("write copy") && next.contains(SPEC_URL), "a resposta não manda copiar a página: {wave2}");
    follow(&project, &wave2);

    let after = copied_items(&project);
    let wave2_id = wave2["id"].as_u64().expect("the wave id");
    assert!(!before.contains(&wave2_id), "a onda nova não podia estar na cópia de antes: {before:?}");
    assert!(after.contains(&wave2_id), "a cópia depois da onda nova não a leva: {after:?}");
    // A cópia leva tudo desde a última cópia gravada: o objetivo, os pontos
    // do levantamento e o plano da onda 1 seguem na faixa, junto da onda
    // nova.
    assert!(after.contains(&said), "a cópia não leva mais o que já estava desde a última cópia gravada: {after:?}");
}
