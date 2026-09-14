//! O levantamento de ponta a ponta pelo binário, num projeto com `git.flow` e
//! um arquivo de código: o `open`, o objetivo, o `grill`, uma resposta e o
//! fechamento de cada ponto, com o passo que cada `write` devolve, a revisão
//! de cada bloco e o fim com as mensagens do usuário sem destino. No meio, com
//! um ponto aberto, a gravação do plano pela porta do binário é recusada com a
//! lista; com todos fechados, ela passa.

use std::path::Path;
use std::process::{Command, Output};

use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::spec_state::{PhaseWriter, State};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::Locale;
use mustard_rt::commands::spec_events::write::record;
use serde_json::{json, Map, Value};

const SPEC: &str = "trava-de-pendencias";
const GOAL: &str = "Travar o merge enquanto houver pendência aberta.";

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?} failed");
}

/// Um repositório com `main` e `dev`, as bases declaradas e um arquivo de
/// código, parado em `dev`. O Mustard fica fora do git.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["checkout", "-q", "-b", "main"]);
    std::fs::write(root.join(".git").join("info").join("exclude"), ".claude/\nmustard.json\n").expect("exclude");
    std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).expect("config");
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("src").join("main.rs"), "fn main() {\n    println!(\"oi\");\n}\n").expect("code");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "init"]);
    git(root, &["checkout", "-q", "-b", "dev"]);
    dir
}

fn rt(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .arg("run")
        .args(args)
        .arg("--root")
        .arg(root)
        .current_dir(root)
        .output()
        .expect("run mustard-rt")
}

fn report(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("not one JSON report ({e}): {}", String::from_utf8_lossy(&out.stdout)))
}

/// Grava pelo `run write` e devolve o relatório; a recusa reprova.
fn write(root: &Path, event_type: &str, fields: &Value) -> Value {
    let out = rt(root, &["write", event_type, "--spec", SPEC, "--json", &fields.to_string()]);
    let written = report(&out);
    assert!(out.status.success(), "write {event_type}: {written}");
    written
}

fn id(report: &Value) -> u64 {
    report["id"].as_u64().unwrap_or_else(|| panic!("no id: {report}"))
}

/// A fase da spec, lida do arquivo de eventos.
fn phase(root: &Path) -> Option<&'static str> {
    let path = store::spec_file(root, SPEC).expect("the spec's file");
    let log = store::read(&path).expect("a readable file").expect("the spec has its file");
    State::from_log(&log).phase
}

/// A gravação da fase de plano pela porta do binário.
fn to_plan(root: &Path) -> Result<(), Refusal> {
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("plan"));
    draft.insert("author".to_string(), json!("binary"));
    record(root, SPEC, "state", draft, PhaseWriter::Binary).map(|_| ())
}

#[test]
fn a_test_survey_goes_through_every_point_and_the_plan_is_refused_while_one_is_open() {
    let dir = repo();
    let root = dir.path();

    let opened = rt(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    assert_eq!(report(&opened)["step"], "ask_goal");

    let said = id(&write(root, "message", &json!({"author": "user", "text": GOAL})));
    write(root, "context", &json!({"text": GOAL, "origin": said}));

    let grilled = rt(root, &["grill", "--kinds", "feature", "--spec", SPEC]);
    assert_eq!(grilled.status.code(), Some(0), "{}", String::from_utf8_lossy(&grilled.stdout));
    let items = report(&grilled)["points"].as_array().cloned().expect("the point list");
    assert_eq!(items.len(), 9, "{items:?}");

    // A lista gravada como o assistente grava: a última gravação devolve o
    // primeiro ponto.
    let mut last = Value::Null;
    for item in &items {
        let mut point = item.clone();
        point["status"] = json!("open");
        point["facts"] = json!([{"text": "O merge começa no arquivo de entrada.", "source": "src/main.rs:2"}]);
        last = write(root, "point", &point);
    }
    let mut current = last["point"].clone();
    assert_eq!(current["gap"], items[0]["gap"], "{last}");

    let loose = [
        id(&write(root, "message", &json!({"author": "user", "text": "E o painel?"}))),
        id(&write(root, "message", &json!({"author": "user", "text": "E o aviso por e-mail?"}))),
    ];

    let mut reviewed = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let point = current["id"].as_u64().expect("the next point");
        let code = current["code"].as_str().expect("the point's code").to_string();
        assert_eq!(current["gap"], item["gap"]);
        let answer = write(
            root,
            "decision",
            &json!({"text": format!("Resposta ao ponto {code}."), "keys": ["levantamento"], "why": "o usuário respondeu",
                "origin": said}),
        );
        assert_eq!(answer["point"]["id"], json!(point), "an answer returns the same point: {answer}");

        if i == items.len() / 2 {
            let refusal = to_plan(root).expect_err("an open point holds the survey");
            assert_eq!(refusal.reason(), "survey-open");
            let listed = refusal.message(Locale::PtBr);
            assert!(listed.contains(&code), "the refusal lists the open point: {listed}");
            assert_eq!(phase(root), Some("survey"), "nothing was written");
        }

        let closed = write(
            root,
            "point",
            &json!({"block": item["block"], "gap": item["gap"], "from": "gap", "status": "closed", "closes": code,
                "result": [id(&answer)], "origin": said}),
        );
        let block_ends = items.get(i + 1).is_none_or(|next| next["block"] != item["block"]);
        if block_ends {
            assert_eq!(closed["review"]["block"], item["block"], "{closed}");
            assert_eq!(closed["review"]["question"], "Quer ver mais algum ponto ou aprofundar algum?");
            reviewed.push(item["block"].clone());
        } else {
            assert!(closed.get("review").is_none(), "{closed}");
        }
        match items.get(i + 1) {
            Some(next) => {
                current = closed["point"].clone();
                assert_eq!(current["gap"], next["gap"], "{closed}");
                assert_eq!(closed["review"].get("options").map(|o| o.as_array().map(Vec::len)), block_ends.then_some(Some(1)));
            }
            None => {
                assert_eq!(
                    closed["review"]["options"],
                    json!(["Quer que um revisor de fora confira o levantamento inteiro?", "Seguir"])
                );
                let unrouted: Vec<u64> =
                    closed["unrouted"].as_array().expect("the end").iter().map(|m| m["id"].as_u64().expect("a number")).collect();
                assert_eq!(unrouted, loose, "{closed}");
            }
        }
    }
    assert_eq!(reviewed.len(), 8, "one review per block: {reviewed:?}");

    assert!(to_plan(root).is_ok(), "with every point closed the passage goes");
    assert_eq!(phase(root), Some("plan"));
}
