// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

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

/// A fala do usuário pelo gancho da entrada, como o Claude Code o chama: o
/// binário lê o evento no `stdin` e grava a fala na spec atual. Devolve o
/// número da fala gravada.
fn hook_says(root: &Path, home: &Path, text: &str) -> u64 {
    use std::io::Write;
    use std::process::Stdio;

    let payload = json!({"hook_event_name": "UserPromptSubmit", "prompt": text, "session_id": "s-levantamento",
        "cwd": root.to_string_lossy()});
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["on", "UserPromptSubmit"])
        .current_dir(root)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("CLAUDE_PROJECT_DIR", root)
        .env("MUSTARD_ACTIVE_SPEC", SPEC)
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CLAUDE_PLUGIN_ROOT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(payload.to_string().as_bytes());
    }
    let out = child.wait_with_output().expect("the binary finishes");
    assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
    let path = store::spec_file(root, SPEC).expect("the spec's file");
    let log = store::read(&path).expect("a readable file").expect("the spec has its file");
    log.visible()
        .into_iter()
        .rfind(|e| e.event_type == "message" && e.str_field("text") == Some(text))
        .unwrap_or_else(|| panic!("the entry hook did not record the message: {text}"))
        .id
}

fn id(report: &Value) -> u64 {
    report["id"].as_u64().unwrap_or_else(|| panic!("no id: {report}"))
}

/// Um critério gravado pelo `run write`, para as tarefas cobrirem: a tarefa
/// do modelo diz os itens que ela entrega.
fn criterion(root: &Path, said: u64) -> u64 {
    id(&write(
        root,
        "criterion",
        &json!({"title": "Somar dois números", "when": "a soma roda", "then": "o total sai certo",
            "proof": "git --version", "form": "ubiquitous", "origin": said}),
    ))
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
    write(root, "context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));

    // Duas falas que chegam sem ponto aberto e sem registro que as aponte.
    let loose = [user_says(root, "E o painel?"), user_says(root, "E o aviso por e-mail?")];

    let grilled = rt(root, &["grill", "--kinds", "feature", "--spec", SPEC]);
    assert_eq!(grilled.status.code(), Some(0), "{}", String::from_utf8_lossy(&grilled.stdout));
    let items = report(&grilled)["points"].as_array().cloned().expect("the point list");
    assert_eq!(items.len(), 9, "{items:?}");

    // O `grill` gravou os pontos; o assistente soma os fatos a cada um, e a
    // última gravação devolve o primeiro ponto.
    let mut last = Value::Null;
    for item in &items {
        let facts = json!([{"text": "O merge começa no arquivo de entrada.", "source": "src/main.rs:2"}]);
        last = write(root, "point", &json!({"replaces": item["id"], "facts": facts}));
    }
    let mut current = last["point"].clone();
    assert_eq!(current["gap"], items[0]["gap"], "{last}");

    let mut reviewed = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let point = current["id"].as_u64().expect("the next point");
        let code = current["code"].as_str().expect("the point's code").to_string();
        assert_eq!(current["gap"], item["gap"]);
        let answer = write(
            root,
            "decision",
            &json!({"title": "Combinar o item", "agent": format!("- ponto {code}"), "text": "O usuário respondeu ao ponto.", "keys": ["levantamento"], "why": "o usuário respondeu",
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
/// depende) é recusada nomeando só a que faltou, e nada entra no arquivo da
/// spec; um texto em branco vale como texto faltando; com as três, sem número
/// de onda e sem nota, a gravação passa.
#[test]
fn a_task_missing_a_declaration_is_refused_naming_only_it_and_writes_nothing() {
    const TEXT: &str = "o que ela faz";
    const FILES: &str = "os arquivos que toca";
    const DEPENDS: &str = "de quais tarefas depende";

    let dir = repo();
    let root = dir.path();
    let opened = rt(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let said = user_says(root, GOAL);
    let crit = criterion(root, said);

    let path = store::spec_file(root, SPEC).expect("the spec's file");
    let lines_before = std::fs::read_to_string(&path).expect("the spec file").lines().count();

    let complete = json!({"agent": "- conferir pelo teste", "title": "Entregar a tarefa", "text": "Somar dois números.", "files": [], "depends_on": [], "covers": [crit], "origin": said});
    let without = |keys: &[&str]| {
        let mut task = complete.clone();
        for key in keys {
            task.as_object_mut().unwrap().remove(*key);
        }
        task
    };
    // O que falta, a tarefa com isso faltando e as declarações que a recusa nomeia.
    let mut blank = complete.clone();
    blank["text"] = json!("   ");
    let cases: [(&str, Value, &[&str]); 4] = [
        ("depends_on", without(&["depends_on"]), &[DEPENDS]),
        ("files and depends_on", without(&["files", "depends_on"]), &[FILES, DEPENDS]),
        ("text", without(&["text"]), &[TEXT]),
        ("a blank text", blank, &[TEXT]),
    ];
    for (missing, task, named) in &cases {
        let out = rt(root, &["write", "task", "--spec", SPEC, "--json", &task.to_string()]);
        let refused = report(&out);
        assert_eq!(refused["reason"], json!("task-declaration-missing"), "{missing}: {refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        for declaration in [TEXT, FILES, DEPENDS] {
            assert_eq!(
                hint.contains(declaration),
                named.contains(&declaration),
                "missing {missing}: the refusal names exactly what is missing: {hint}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(&path).expect("the spec file").lines().count(),
            lines_before,
            "missing {missing}: nothing was written"
        );
    }

    let written = write(root, "task", &complete);
    assert_eq!(written["ok"], json!(true), "{written}");
    assert!(written.get("id").is_some(), "{written}");
    assert_eq!(std::fs::read_to_string(&path).expect("the spec file").lines().count(), lines_before + 1);
}

/// A pergunta feita no meio de um ponto aberto, pelo gancho da entrada, leva
/// o número do ponto e não fica solta no fim do levantamento, embora o ponto
/// feche com uma decisão que não a cita; a fala que chegou sem ponto aberto
/// continua solta, e a fala pelo gancho com os pontos já fechados também.
#[test]
fn a_question_said_during_an_open_point_is_not_left_loose_at_the_end() {
    let dir = repo();
    let home = tempfile::tempdir().expect("home");
    let root = dir.path();
    let opened = rt(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let said = user_says(root, GOAL);
    write(root, "context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));
    let before_points = hook_says(root, home.path(), "E o painel?");

    let items = report(&rt(root, &["grill", "--kinds", "feature", "--spec", SPEC]))["points"]
        .as_array()
        .cloned()
        .expect("the point list");
    let mut last = Value::Null;
    for item in &items {
        let facts = json!([{"text": "O merge começa no arquivo de entrada.", "source": "src/main.rs:2"}]);
        last = write(root, "point", &json!({"replaces": item["id"], "facts": facts}));
    }
    let mut current = last["point"].clone();
    let first_open = current["id"].as_u64().expect("the first open point");
    let question = hook_says(root, home.path(), "8 é fixo?");

    let path = store::spec_file(root, SPEC).expect("the spec's file");
    let log = store::read(&path).expect("a readable file").expect("the spec has its file");
    let said_during = |id: u64| log.get(id).and_then(|m| m.int("during"));
    assert_eq!(said_during(question), Some(first_open), "the question carries the point that was open");
    assert_eq!(said_during(before_points), None, "a message without an open point carries nothing");

    let mut ended = Value::Null;
    for item in &items {
        let code = current["code"].as_str().expect("the point's code").to_string();
        let answer = write(
            root,
            "decision",
            &json!({"title": "Combinar o item", "agent": format!("- ponto {code}"), "text": "O usuário respondeu ao ponto.", "keys": ["levantamento"], "why": "o usuário respondeu", "origin": said}),
        );
        let closed = write(
            root,
            "point",
            &json!({"block": item["block"], "gap": item["gap"], "from": "gap", "status": "closed", "closes": code,
                "result": [id(&answer)], "origin": said}),
        );
        current = closed["point"].clone();
        ended = closed;
    }
    let unrouted: Vec<u64> =
        ended["unrouted"].as_array().expect("the end of the survey").iter().map(|m| m["id"].as_u64().expect("a number")).collect();
    assert_eq!(unrouted, [before_points], "only the message that came with no open point is left loose: {ended}");

    let late = hook_says(root, home.path(), "E agora?");
    let log = store::read(&path).expect("a readable file").expect("the spec has its file");
    assert_eq!(log.get(late).and_then(|m| m.int("during")), None, "with every point closed nothing is carried");
}

/// Duas tarefas que passam a depender uma da outra, em círculo, são
/// recusadas nomeando o círculo inteiro, na ordem, com os códigos das
/// tarefas; nada é gravado.
#[test]
fn cycle_between_tasks_is_refused_naming_the_cycle() {
    let dir = repo();
    let root = dir.path();
    rt(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    let said = user_says(root, GOAL);
    let crit = criterion(root, said);

    let a = write(root, "task", &json!({"agent": "- conferir pelo teste", "title": "Entregar a tarefa", "text": "Tarefa A.", "files": [], "depends_on": [], "covers": [crit], "origin": said}));
    let a_id = id(&a);
    let a_code = a["code"].as_str().unwrap().to_string();

    let b = write(
        root,
        "task",
        &json!({"agent": "- conferir pelo teste", "title": "Entregar a tarefa", "text": "Tarefa B.", "files": [], "depends_on": [a_code.clone()], "covers": [crit], "origin": said}),
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
            &json!({"agent": "- conferir pelo teste",
                "title": "Entregar a tarefa", "text": "Tarefa A, revista.", "files": [], "depends_on": [b_code.clone()], "covers": [crit],
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
        &json!({"agent": "- conferir pelo teste", "title": "Entregar a tarefa", "text": "Tarefa A, revista.", "files": [], "depends_on": [], "covers": [crit], "replaces": a_id, "origin": said}),
    );
    assert_eq!(revised["code"], json!(a_code), "{revised}");
}

/// Uma tarefa que declara depender de uma tarefa que não existe nesta spec é
/// recusada, nomeando as duas: a que declarou a dependência e a que não
/// existe; nada é gravado. A revisão de uma tarefa existente, pelo
/// `replaces`, dá à declarante um código conhecido do teste — o mesmo jeito
/// que o teste do círculo, logo acima, já usa para nomear os dois lados.
#[test]
fn dependency_on_a_missing_task_is_refused() {
    let dir = repo();
    let root = dir.path();
    rt(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    let said = user_says(root, GOAL);
    let crit = criterion(root, said);

    let a = write(root, "task", &json!({"agent": "- conferir pelo teste", "title": "Entregar a tarefa", "text": "Tarefa A.", "files": [], "depends_on": [], "covers": [crit], "origin": said}));
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
            &json!({"agent": "- conferir pelo teste",
                "title": "Entregar a tarefa", "text": "Tarefa A, revista.", "files": [], "depends_on": ["MSTD-TASK-0099"], "covers": [crit],
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

/// A tarefa que o modelo grava sem dizer os itens que ela cobre, sem a chave
/// ou com a lista vazia, é recusada, e nada entra no arquivo da spec: a onda
/// leva como critérios os itens que as tarefas cobrem, e sem eles a rodada
/// só recusaria a onda depois. A recusa dá a dica de qual item cobrir, pelo
/// número e pelo código: os que nenhuma tarefa cobre ainda. Com o item, a
/// tarefa grava, e o item coberto sai da dica da próxima recusa. O item que
/// vale no projeto todo e o que não vira código nunca entram na dica.
#[test]
fn a_task_without_the_items_it_covers_is_refused_naming_the_items_no_task_covers() {
    let dir = repo();
    let root = dir.path();
    let opened = rt(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let said = user_says(root, GOAL);
    let criterion = |title: &str| {
        let written = write(
            root,
            "criterion",
            &json!({"title": title, "when": "a soma roda", "then": "o total sai certo",
                "proof": "git --version", "form": "ubiquitous", "origin": said}),
        );
        (id(&written), written["code"].as_str().unwrap_or_else(|| panic!("no code: {written}")).to_string())
    };
    let (one, one_code) = criterion("Somar dois números");
    let (two, two_code) = criterion("Somar três números");
    let task = |covers: Option<Value>| {
        let mut body = json!({"agent": "- conferir pelo teste", "title": "Entregar a soma", "text": "Somar os números.",
            "files": [], "depends_on": [], "origin": said});
        if let Some(covers) = covers {
            body["covers"] = covers;
        }
        body
    };
    let path = store::spec_file(root, SPEC).expect("the spec's file");
    // O item que vale no projeto todo e o que não vira código não pedem
    // tarefa: ficam fora da dica.
    let rule = |extra: Value| {
        let mut draft = json!({"text": "A soma usa inteiros.", "keys": ["soma"], "example": "1 + 2 = 3", "origin": said});
        draft.as_object_mut().expect("object").extend(extra.as_object().cloned().unwrap_or_default());
        let written = store::write(&path, "rule", draft.as_object().cloned().unwrap_or_default(), &[]).expect("the rule");
        written.code.expect("the rule's code")
    };
    let everywhere = rule(json!({"applies_to": {"files": ["**"]}}));
    let no_code = rule(json!({"no_code": "É uma convenção de escrita."}));
    let lines = || std::fs::read_to_string(&path).expect("the spec file").lines().count();
    let refused = |body: &Value, case: &str| -> String {
        let before = lines();
        let out = rt(root, &["write", "task", "--spec", SPEC, "--json", &body.to_string()]);
        let refused = report(&out);
        assert!(!out.status.success(), "{case}: {refused}");
        assert_eq!(refused["reason"], json!("task-declaration-missing"), "{case}: {refused}");
        assert_eq!(lines(), before, "{case}: nothing was written");
        refused["hint"].as_str().unwrap_or_default().to_string()
    };

    for (covers, case) in [(None, "no covers"), (Some(json!([])), "empty covers")] {
        let hint = refused(&task(covers), case);
        assert!(hint.contains("`covers`"), "{case}: the refusal names the field: {hint}");
        assert!(!hint.contains("de quais tarefas depende") && !hint.contains("os arquivos que toca"), "{case}: {hint}");
        let (first, second) = (format!("{one} ({one_code})"), format!("{two} ({two_code})"));
        assert!(hint.contains(&first) && hint.contains(&second), "{case}: the hint names both items: {hint}");
        assert!(!hint.contains(&everywhere) && !hint.contains(&no_code), "{case}: no task owes these: {hint}");
    }

    let before = lines();
    let written = write(root, "task", &task(Some(json!([one]))));
    assert_eq!(written["ok"], json!(true), "{written}");
    assert_eq!(lines(), before + 1, "the task that covers an item is written");

    let hint = refused(&task(None), "after one is covered");
    assert!(hint.contains(&format!("{two} ({two_code})")), "the item no task covers stays: {hint}");
    assert!(!hint.contains(&one_code), "the covered item leaves the hint: {hint}");
}
