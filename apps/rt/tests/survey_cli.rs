//! O levantamento de ponta a ponta pelo binário, num projeto com `git.flow` e
//! um arquivo de código: o `open`, o objetivo, o `grill`, uma resposta e o
//! fechamento de cada ponto, com o passo que cada `write` devolve, a revisão
//! de cada bloco, a ordem de rodar o revisor de fora e o fim com as mensagens
//! do usuário sem destino. No meio, com
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

/// A fala do usuário, gravada como o gancho da entrada a grava: o `run write`
/// não grava mensagem do usuário.
fn user_says(root: &Path, text: &str) -> u64 {
    let mut draft = Map::new();
    draft.insert("author".to_string(), json!("user"));
    draft.insert("text".to_string(), json!(text));
    record(root, SPEC, "message", draft, PhaseWriter::Binary).expect("the hook records the message");
    let path = store::spec_file(root, SPEC).expect("the spec's file");
    let log = store::read(&path).expect("a readable file").expect("the spec has its file");
    log.events.last().expect("the message just written").id
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

    let said = user_says(root, GOAL);
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

    let loose = [user_says(root, "E o painel?"), user_says(root, "E o aviso por e-mail?")];

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
                assert_eq!(closed["review"]["options"], json!(["Seguir"]), "the outside reviewer is not an option");
                let next = closed["next"].as_str().expect("the next step");
                assert!(
                    next.contains("rode o revisor de fora") && next.contains("`mustard-review`") && next.contains(SPEC),
                    "the end of the survey orders the outside reviewer: {next}"
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

/// A tarefa gravada pela porta do binário sem uma das três declarações
/// obrigatórias (o que ela faz, os arquivos que toca, de quais tarefas
/// depende) é recusada nomeando exatamente a que faltou, e nada entra no
/// arquivo da spec; com as três, a gravação passa.
#[test]
fn a_task_missing_one_of_the_three_declarations_is_refused_naming_it_and_writes_nothing() {
    let dir = repo();
    let root = dir.path();
    let opened = rt(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let said = user_says(root, GOAL);

    let path = store::spec_file(root, SPEC).expect("the spec's file");
    let lines_before = std::fs::read_to_string(&path).expect("the spec file").lines().count();

    // Falta só `depends_on`: a recusa nomeia só ela.
    let out = rt(
        root,
        &[
            "write",
            "task",
            "--spec",
            SPEC,
            "--json",
            &json!({"wave": 1, "text": "Somar dois números.", "files": [], "origin": said}).to_string(),
        ],
    );
    let refused = report(&out);
    assert_eq!(refused["reason"], json!("task-declaration-missing"), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    assert!(hint.contains("de quais tarefas depende"), "{hint}");
    assert!(!hint.contains("o que ela faz") && !hint.contains("os arquivos que toca"), "{hint}");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the spec file").lines().count(),
        lines_before,
        "nothing was written"
    );

    // Faltam `files` e `depends_on`: a recusa nomeia as duas, sem citar `text`.
    let out = rt(
        root,
        &[
            "write",
            "task",
            "--spec",
            SPEC,
            "--json",
            &json!({"wave": 1, "text": "Somar dois números.", "origin": said}).to_string(),
        ],
    );
    let refused = report(&out);
    assert_eq!(refused["reason"], json!("task-declaration-missing"), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    assert!(hint.contains("os arquivos que toca") && hint.contains("de quais tarefas depende"), "{hint}");
    assert!(!hint.contains("o que ela faz"), "{hint}");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the spec file").lines().count(),
        lines_before,
        "nothing was written"
    );

    // Com as três, a gravação passa.
    let written = write(
        root,
        "task",
        &json!({"wave": 1, "text": "Somar dois números.", "files": [], "depends_on": [], "origin": said}),
    );
    assert!(written.get("id").is_some(), "{written}");
    assert_eq!(std::fs::read_to_string(&path).expect("the spec file").lines().count(), lines_before + 1);
}

/// A tarefa gravada só com texto, arquivos e dependências, sem número de
/// onda e sem nota, é aceita.
#[test]
fn uma_tarefa_sem_numero_de_onda_e_gravada() {
    let dir = repo();
    let root = dir.path();
    rt(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    let said = user_says(root, GOAL);

    let written = write(
        root,
        "task",
        &json!({"text": "Somar dois números.", "files": [], "depends_on": [], "origin": said}),
    );
    assert_eq!(written["ok"], json!(true), "{written}");
    assert!(written.get("id").is_some(), "{written}");
}

/// Duas tarefas que passam a depender uma da outra, em círculo, são
/// recusadas nomeando o círculo inteiro, na ordem, com os códigos das
/// tarefas; nada é gravado.
#[test]
fn o_circulo_entre_tarefas_e_recusado_nomeando_o_circulo() {
    let dir = repo();
    let root = dir.path();
    rt(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    let said = user_says(root, GOAL);

    let a = write(root, "task", &json!({"text": "Tarefa A.", "files": [], "depends_on": [], "origin": said}));
    let a_id = id(&a);
    let a_code = a["code"].as_str().unwrap().to_string();

    let b = write(
        root,
        "task",
        &json!({"text": "Tarefa B.", "files": [], "depends_on": [a_code.clone()], "origin": said}),
    );
    let b_code = b["code"].as_str().unwrap().to_string();

    let path = store::spec_file(root, SPEC).expect("the spec's file");
    let lines_before = std::fs::read_to_string(&path).expect("the spec file").lines().count();

    // A revisão de A passa a depender de B, que já depende de A: círculo.
    let out = rt(
        root,
        &[
            "write",
            "task",
            "--spec",
            SPEC,
            "--json",
            &json!({
                "text": "Tarefa A, revista.", "files": [], "depends_on": [b_code.clone()],
                "replaces": a_id, "origin": said,
            })
            .to_string(),
        ],
    );
    let refused = report(&out);
    assert_eq!(refused["reason"], json!("task-dependency-cycle"), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    assert!(hint.contains(&a_code) && hint.contains(&b_code), "{hint}");
    let a_at = hint.find(&a_code).unwrap();
    let b_at = hint.find(&b_code).unwrap();
    assert!(a_at < b_at, "o círculo nomeia A antes de B: {hint}");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the spec file").lines().count(),
        lines_before,
        "nothing was written"
    );

    // Sem o círculo, a revisão de A passa.
    let revised = write(
        root,
        "task",
        &json!({"text": "Tarefa A, revista.", "files": [], "depends_on": [], "replaces": a_id, "origin": said}),
    );
    assert_eq!(revised["code"], json!(a_code), "{revised}");
}

/// Uma tarefa que declara depender de uma tarefa que não existe nesta spec é
/// recusada, nomeando as duas: a que declarou a dependência e a que não
/// existe; nada é gravado. A revisão de uma tarefa existente, pelo
/// `replaces`, dá à declarante um código conhecido do teste — o mesmo jeito
/// que o teste do círculo, logo acima, já usa para nomear os dois lados.
#[test]
fn a_dependencia_de_tarefa_inexistente_e_recusada() {
    let dir = repo();
    let root = dir.path();
    rt(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    let said = user_says(root, GOAL);

    let a = write(root, "task", &json!({"text": "Tarefa A.", "files": [], "depends_on": [], "origin": said}));
    let a_id = a["id"].as_u64().unwrap();
    let a_code = a["code"].as_str().unwrap().to_string();

    let path = store::spec_file(root, SPEC).expect("the spec's file");
    let lines_before = std::fs::read_to_string(&path).expect("the spec file").lines().count();

    let out = rt(
        root,
        &[
            "write",
            "task",
            "--spec",
            SPEC,
            "--json",
            &json!({
                "text": "Tarefa A, revista.", "files": [], "depends_on": ["MSTD-TASK-0099"],
                "replaces": a_id, "origin": said,
            })
            .to_string(),
        ],
    );
    let refused = report(&out);
    assert_eq!(refused["reason"], json!("task-depends-on-unknown"), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    assert!(hint.contains(&a_code), "the message names the task that declared the dependency: {hint}");
    assert!(hint.contains("MSTD-TASK-0099"), "the message names the dependency that does not exist: {hint}");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the spec file").lines().count(),
        lines_before,
        "nothing was written"
    );
}
