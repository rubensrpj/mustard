// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A cesta despachada pelo binário de verdade, numa pasta temporária: seis
//! tarefas com dependência e arquivo compartilhado, e os lotes que a rodada
//! grava, como onda de autor `binary`, sempre saem os mesmos — quem
//! compartilha arquivo cai junto, ninguém divide arquivo entre lotes, e a
//! capacidade de cinco arquivos fecha o primeiro lote antes do segundo abrir.
//!
//! Move a prova que antes vivia só como teste de unidade do módulo do grafo
//! (`apps/rt/src/shared/dag.rs`): o critério fala em despacho pelo binário,
//! num repositório temporário, não em chamar a função pura duas vezes.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::State;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

const SPEC: &str = "cesta-lotes";
const GOAL: &str = "Trocar a saudação do programa.";
const SESSION: &str = "s-cesta-lotes";

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

/// Uma tarefa da cesta, gravada sem onda: `files` como caminhos soltos e
/// `depends_on` como a lista de ids das tarefas de que ela depende.
/// Devolve o `id` gravado.
fn basket_task(project: &Project, criterion: u64, said: u64, files: &[&str], depends_on: &[u64]) -> u64 {
    // `"new": true` marca um arquivo que a tarefa ainda vai criar: sem isso
    // o plano recusa a pergunta de aprovação, porque o arquivo sintético do
    // teste não existe no repositório.
    let files: Vec<Value> = files.iter().map(|f| json!({"path": f, "new": true})).collect();
    let depends_on: Vec<Value> = depends_on.iter().map(|id| json!(id)).collect();
    let written = project.write(
        "task",
        &json!({"text": "Tarefa da cesta.", "files": files, "depends_on": depends_on,
            "covers": [criterion], "origin": said}),
    );
    written["id"].as_u64().expect("the recorded task has an id")
}

/// A cesta inteira, numa spec de seis tarefas com dependências e arquivos
/// declarados, despachada pelo binário de verdade: o nível topológico, a
/// prontidão com desempate e o empacotamento sempre devolvem os mesmos
/// lotes — o mesmo grafo do teste que antes vivia só como unidade pura do
/// módulo do grafo (`apps/rt/src/shared/dag.rs`), agora conferido na saída
/// de uma rodada de verdade.
#[test]
fn a_cesta_de_tarefas_vira_sempre_os_mesmos_lotes() {
    let project = Project::new();
    project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    let said = survey(&project);
    let criterion = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "origin": said}),
    );
    let crit_id = criterion["id"].as_u64().expect("the criterion has an id");

    // O mesmo grafo do teste puro: 1 e 5 dividem `b.rs`; 4 é a maior parte,
    // sozinha; 2 não compartilha arquivo com ninguém; 3 espera 1, 6 espera 4.
    let t1 = basket_task(&project, crit_id, said, &["a.rs", "b.rs"], &[]);
    let t2 = basket_task(&project, crit_id, said, &["c.rs"], &[]);
    let t3 = basket_task(&project, crit_id, said, &["d.rs"], &[t1]);
    let t4 = basket_task(&project, crit_id, said, &["e.rs", "f.rs", "g.rs"], &[]);
    let t5 = basket_task(&project, crit_id, said, &["b.rs"], &[]);
    let t6 = basket_task(&project, crit_id, said, &["h.rs"], &[t4]);

    project.run(&["plan", "--spec", SPEC]);
    approve(&project);

    let before = project.log();
    assert!(before.visible().into_iter().all(|e| e.event_type != "wave"), "no hand-made wave before the round");

    project.run(&["round", "--spec", SPEC]);

    let after = project.log();
    let waves: Vec<_> =
        after.visible().into_iter().filter(|e| e.event_type == "wave" && e.str_field("author") == Some("binary")).collect();
    assert_eq!(waves.len(), 2, "1, 4 e 5 num lote, 2 sozinho no outro; 3 e 6 esperam dependência aberta: {waves:?}");

    let order_of = |w: &mustard_core::domain::spec_events::SpecEvent| -> BTreeSet<u64> {
        w.fields.get("order").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect()
    };
    let big = waves.iter().find(|w| order_of(w).len() == 3).expect("o lote de três tarefas");
    let small = waves.iter().find(|w| order_of(w).len() == 1).expect("o lote de uma tarefa só");

    assert_eq!(order_of(big), BTreeSet::from([t1, t4, t5]), "1, 4 e 5 dividem arquivo ou cabem juntos até a capacidade de 5");
    assert_eq!(order_of(small), BTreeSet::from([t2]), "2 não compartilha arquivo com a parte de 1, 4 e 5");

    // 3 e 6 esperam a dependência aberta: nenhuma das duas ondas os leva.
    assert!(!order_of(big).contains(&t3) && !order_of(small).contains(&t3), "3 espera 1, ainda aberta");
    assert!(!order_of(big).contains(&t6) && !order_of(small).contains(&t6), "6 espera 4, ainda aberta");

    // A capacidade de 5 arquivos fecha o lote maior antes do menor abrir: o
    // lote com mais arquivos distintos (1, 4 e 5, com 5 arquivos ao todo)
    // sai antes do lote com menos (2, com 1 arquivo só).
    let big_n = big.int("n").expect("wave n");
    let small_n = small.int("n").expect("wave n");
    assert!(big_n < small_n, "o lote maior abre antes do menor: {big_n} vs {small_n}");
}
