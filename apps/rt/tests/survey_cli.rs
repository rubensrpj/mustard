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

#[path = "support/binary.rs"]
mod binary;

use std::path::Path;
use std::process::{Command, Output};

use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::spec_state::{PhaseWriter, State};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::Locale;
use mustard_rt::commands::spec_events::write::record;
use serde_json::{json, Map, Value};

const SPEC: &str = "trava-de-pendencias";
const SESSION: &str = "s-levantamento";
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
    binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
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

/// A abertura da spec `name` pela conversa da sessão [`SESSION`], como quem
/// conduz a roda: o gancho do terminal passa a sessão ao comando, e é a conversa
/// dela que fica ligada à spec, a mesma que fala depois pelo gancho.
fn open(root: &Path, kind: &str, name: &str) -> Output {
    binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "open", "--kind", kind, "--name", name, "--base", "dev", "--root"])
        .arg(root)
        .current_dir(root)
        .env("MUSTARD_SESSION_ID", SESSION)
        .output()
        .expect("run mustard-rt")
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

/// Um gancho do Claude Code pelo binário, com o JSON do evento no `stdin`, na
/// sessão do levantamento. A spec é a que a abertura ligou a essa sessão: nada
/// a força pelo ambiente. Devolve a saída do gancho, que sempre sai com 0.
fn on(root: &Path, home: &Path, event: &str, payload: &Value) -> String {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["on", event])
        .current_dir(root)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("CLAUDE_PROJECT_DIR", root)
        .env_remove("MUSTARD_ACTIVE_SPEC")
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
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A fala do usuário pelo gancho da entrada, como o Claude Code o chama: o
/// binário lê o evento no `stdin` e grava a fala na spec atual. Devolve o
/// número da fala gravada.
fn hook_says(root: &Path, home: &Path, text: &str) -> u64 {
    let payload = json!({"hook_event_name": "UserPromptSubmit", "prompt": text, "session_id": SESSION,
        "cwd": root.to_string_lossy()});
    on(root, home, "UserPromptSubmit", &payload);
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

/// O código que a spec dá ao item de número `id`.
fn code_of(root: &Path, id: u64) -> String {
    let path = store::spec_file(root, SPEC).expect("the spec's file");
    let log = store::read(&path).expect("a readable file").expect("the spec has its file");
    log.codes().get(&id).cloned().unwrap_or_else(|| panic!("no code for {id}"))
}

/// As linhas que o relatório traz em `unrouted`: o código, dois pontos e o
/// começo do texto da mensagem, numa linha só.
fn unrouted_lines(report: &Value) -> Vec<String> {
    let lines = report["unrouted"].as_array().unwrap_or_else(|| panic!("no unrouted: {report}"));
    lines.iter().map(|line| line.as_str().unwrap_or_else(|| panic!("not a line: {line}")).to_string()).collect()
}

/// Os códigos das mensagens que o relatório lista em `unrouted`.
fn unrouted_codes(report: &Value) -> Vec<String> {
    unrouted_lines(report)
        .into_iter()
        .map(|line| line.split_once(": ").map(|(code, _)| code.to_string()).unwrap_or_else(|| panic!("not code and start: {line}")))
        .collect()
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

/// Grava pelo `run write` numa spec qualquer; a recusa reprova.
fn write_in(root: &Path, spec: &str, event_type: &str, fields: &Value) -> Value {
    let out = rt(root, &["write", event_type, "--spec", spec, "--json", &fields.to_string()]);
    let written = report(&out);
    assert!(out.status.success(), "write {event_type} in {spec}: {written}");
    written
}

/// A fala do usuário numa spec qualquer, gravada como o gancho da entrada a
/// grava. Devolve o número dela.
fn says_in(root: &Path, spec: &str, text: &str) -> u64 {
    let mut draft = Map::new();
    draft.insert("author".to_string(), json!("user"));
    draft.insert("text".to_string(), json!(text));
    record(root, spec, "message", draft, PhaseWriter::Binary).expect("the hook records the message");
    let path = store::spec_file(root, spec).expect("the spec's file");
    let log = store::read(&path).expect("a readable file").expect("the spec has its file");
    log.events.last().expect("the message just written").id
}

/// Abre uma spec de conserto pelo `open` do binário e grava o objetivo dela:
/// `text` para o usuário, com o título que é a frase sem o ponto, e `agent`
/// para quem implementa.
fn open_with_goal(root: &Path, name: &str, text: &str, agent: &str) {
    let opened = open(root, "fix", name);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let said = says_in(root, name, text);
    write_in(root, name, "context", &json!({"title": text.trim_end_matches('.'), "agent": agent, "text": text, "origin": said}));
}

/// Uma spec anterior, escrita pelo binário: o objetivo, uma regra que cita
/// `rule_agent` e, quando há arquivos, a entrega de uma onda que mexeu neles.
fn earlier_spec(root: &Path, name: &str, goal: &str, rule_agent: &str, delivered: &[&str]) {
    open_with_goal(root, name, goal, "- o que o objetivo pede");
    let said = says_in(root, name, "A regra vale aqui.");
    write_in(
        root,
        name,
        "rule",
        &json!({"title": "Regra do trecho", "text": "Vale no trecho.", "keys": ["trecho"], "example": "um exemplo", "agent": rule_agent, "origin": said}),
    );
    if !delivered.is_empty() {
        // A entrega só nasce depois de uma onda enviada e de um agente que a
        // devolve; a spec anterior chega com ela já escrita no arquivo.
        let path = store::spec_file(root, name).expect("the spec's file");
        let draft = json!({"wave": 1, "text": "A onda saiu.", "files": delivered});
        store::write(&path, "delivered", draft.as_object().expect("an object").clone(), &[]).expect("the delivery lands in the file");
    }
}

/// O levantamento da spec nova, e os nomes das specs anteriores que viraram
/// ponto dele, na ordem da lista.
fn earlier_specs_surveyed(root: &Path) -> Vec<String> {
    let grilled = rt(root, &["grill", "--kinds", "fix", "--spec", "nova", "--full"]);
    assert_eq!(grilled.status.code(), Some(0), "{}", String::from_utf8_lossy(&grilled.stdout));
    let answer = report(&grilled);
    answer["points"]
        .as_array()
        .expect("the point list")
        .iter()
        .filter(|point| point["from"] == json!("prior_spec"))
        .map(|point| point["gap"].as_str().and_then(|gap| gap.split(':').next()).expect("the gap").to_string())
        .collect()
}

/// A spec nova que cita um arquivo na parte do agente do objetivo (o texto
/// do usuário não leva caminho) leva ao levantamento a spec
/// anterior que mexeu nesse arquivo, seja pelo arquivo da entrega, seja pelo
/// citado numa regra dela, ainda que nenhuma palavra do objetivo case. A
/// spec anterior que só divide duas palavras com o objetivo fica de fora, por
/// mexer em outro arquivo.
#[test]
fn an_earlier_spec_enters_the_survey_by_a_file_in_common_and_not_by_two_words() {
    let dir = repo();
    let root = dir.path();
    earlier_spec(root, "impressao", "Reescrever a rotina de impressão.", "- a rotina mora em `src/other.rs`", &["src/main.rs"]);
    earlier_spec(root, "etiqueta", "Redesenhar o rótulo.", "- a saída sai de `src/main.rs:2`", &[]);
    earlier_spec(root, "pendencias", "Travar o merge com pendência aberta.", "- vive em `src/lib.rs`", &["src/lib.rs"]);
    open_with_goal(root, "nova", "Travar o merge com pendência aberta.", "- o defeito está em `src/main.rs:2`");

    let entered = earlier_specs_surveyed(root);
    assert_eq!(entered, ["etiqueta", "impressao"], "{entered:?}");
}

/// Sem arquivo citado no objetivo, a escolha é a de sempre: entra a spec
/// anterior que divide duas palavras com ele, e a que só mexeu num arquivo
/// qualquer fica de fora.
#[test]
fn without_a_file_cited_an_earlier_spec_still_enters_by_two_words() {
    let dir = repo();
    let root = dir.path();
    earlier_spec(root, "impressao", "Reescrever a rotina de impressão.", "- a rotina mora em `src/other.rs`", &["src/main.rs"]);
    earlier_spec(root, "pendencias", "Travar o merge com pendência aberta.", "- vive em `src/lib.rs`", &["src/lib.rs"]);
    open_with_goal(root, "nova", "Travar o merge com pendência aberta.", "- conferir pelo teste");

    let entered = earlier_specs_surveyed(root);
    assert_eq!(entered, ["pendencias"], "{entered:?}");
}

#[test]
fn a_test_survey_goes_through_every_point_and_the_plan_is_refused_while_one_is_open() {
    let dir = repo();
    let root = dir.path();

    let opened = open(root, "feature", SPEC);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    assert_eq!(report(&opened)["step"], "ask_goal");

    let said = user_says(root, GOAL);
    write(root, "context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));

    // Duas falas que chegam sem ponto aberto e sem registro que as aponte.
    let loose = [user_says(root, "E o painel?"), user_says(root, "E o aviso por e-mail?")];

    let grilled = rt(root, &["grill", "--kinds", "feature", "--spec", SPEC, "--full"]);
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

    for (i, item) in items.iter().enumerate() {
        assert!(current["id"].is_u64(), "the next point: {current}");
        let code = current["code"].as_str().expect("the point's code").to_string();
        assert_eq!(current["gap"], item["gap"]);
        let answer = write(
            root,
            "decision",
            &json!({"title": "Combinar o item", "agent": format!("- ponto {code}"), "text": "O usuário respondeu ao ponto.", "keys": ["levantamento"], "why": "o usuário respondeu",
                "origin": said}),
        );
        assert!(answer.get("point").is_none(), "an answer that closes no point does not repeat the open one: {answer}");

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
        assert!(closed.get("review").is_none(), "no review per block: {closed}");
        match items.get(i + 1) {
            Some(next) => {
                current = closed["point"].clone();
                assert_eq!(current["gap"], next["gap"], "{closed}");
                let open = closed["points"].as_array().map_or(0, Vec::len);
                assert_eq!(open, items.len() - i - 1, "the open points come together: {closed}");
            }
            None => {
                let next = closed["next"].as_str().expect("the next step");
                assert!(!next.contains("`mustard-review`"), "the survey orders no outside reviewer: {next}");
                let loose_codes: Vec<String> = loose.iter().map(|id| code_of(root, *id)).collect();
                assert_eq!(unrouted_codes(&closed), loose_codes, "{closed}");
            }
        }
    }

    assert!(to_plan(root).is_ok(), "with every point closed the passage goes");
    assert_eq!(phase(root), Some("plan"));
}

/// O levantamento inteiro sai com uma chamada de `run answer` por ponto: a
/// resposta num item novo, sem `--point` e sem `origin`, fecha o primeiro
/// ponto aberto e aponta a última fala do usuário; a resposta pelo item já
/// gravado e o "não se aplica" com o motivo fecham o ponto apontado; cada
/// chamada devolve o próximo ponto. O ponto que já fechou é recusado com
/// saída 1, e nada além da chamada é gravado.
#[test]
fn a_survey_is_answered_with_one_answer_call_per_point() {
    let dir = repo();
    let root = dir.path();
    let opened = open(root, "feature", SPEC);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let said = user_says(root, GOAL);
    write(root, "context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));
    let items = report(&rt(root, &["grill", "--kinds", "feature", "--spec", SPEC, "--full"]))["points"]
        .as_array()
        .cloned()
        .expect("the point list");
    let mut current = Value::Null;
    for item in &items {
        let facts = json!([{"text": "O merge começa no arquivo de entrada.", "source": "src/main.rs:2"}]);
        current = write(root, "point", &json!({"replaces": item["id"], "facts": facts}))["point"].clone();
    }
    let reply = user_says(root, "Vale a mesma regra para todos os pontos.");
    let decision = json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": "O usuário respondeu ao ponto.",
        "keys": ["levantamento"], "why": "o usuário respondeu"})
    .to_string();
    let path = store::spec_file(root, SPEC).expect("the spec's file");
    // O que a spec guarda além do registro de cada chamada, que toda chamada
    // de um passo do fluxo grava, recusada ou não.
    let recorded = || -> Vec<u64> {
        let log = store::read(&path).expect("a readable file").expect("the spec has its file");
        log.events.iter().filter(|e| e.event_type != "call").map(|e| e.id).collect()
    };

    let (mut first_item, mut first_id) = (String::new(), 0);
    for (i, item) in items.iter().enumerate() {
        let point = current["id"].as_u64().expect("the next point");
        let code = current["code"].as_str().expect("the point's code").to_string();
        assert_eq!(current["gap"], item["gap"], "{current}");
        let shown = point.to_string();
        let args: Vec<&str> = match i {
            0 => vec!["answer", "--spec", SPEC, "--type", "decision", "--json", &decision],
            1 => vec!["answer", "--spec", SPEC, "--point", &code, "--result", &first_item],
            2 => vec!["answer", "--spec", SPEC, "--point", &shown, "--not-applicable", "--reason", "Não se aplica."],
            _ => vec!["answer", "--spec", SPEC, "--point", &code, "--type", "decision", "--json", &decision],
        };
        let out = rt(root, &args);
        let answered = report(&out);
        assert_eq!(out.status.code(), Some(0), "answer {i}: {answered}");
        assert_eq!(answered["closed"]["closes"], json!(point), "{answered}");
        let log = store::read(&path).expect("a readable file").expect("the spec has its file");
        match i {
            0 => {
                first_id = id(&answered);
                assert_eq!(log.get(first_id).and_then(|e| e.int("origin")), Some(reply), "{answered}");
                assert_eq!(answered["closed"]["origin"], json!(reply), "{answered}");
                first_item = answered["code"].as_str().expect("the item's code").to_string();
            }
            1 => assert_eq!(answered["closed"]["result"], json!([first_id]), "{answered}"),
            2 => {
                assert!(answered.get("id").is_none(), "no item: {answered}");
                assert_eq!(answered["closed"]["status"], json!("not_applicable"), "{answered}");
                assert_eq!(answered["closed"]["reason"], json!("Não se aplica."), "{answered}");
                let before = recorded();
                let again = rt(root, &["answer", "--spec", SPEC, "--point", &shown, "--result", &first_item]);
                assert_eq!(again.status.code(), Some(1), "a closed point is refused with exit 1");
                assert_eq!(report(&again)["reason"], json!("point-not-open"));
                assert_eq!(recorded(), before, "a refusal writes no item and no point");
            }
            _ => {}
        }
        current = answered["point"].clone();
        match items.get(i + 1) {
            Some(next) => {
                assert_eq!(current["gap"], next["gap"], "the answer returns the next point: {answered}");
                let said = answered["next"].as_str().unwrap_or_default();
                assert!(said.contains("mustard-rt run answer --point"), "the next step gives the call that answers each point: {said}");
            }
            None => assert!(current.is_null() && answered.get("unrouted").is_some(), "the end of the survey: {answered}"),
        }
    }
    assert!(to_plan(root).is_ok(), "with every point closed the passage goes");
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
    let opened = open(root, "feature", SPEC);
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
    let opened = open(root, "feature", SPEC);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let said = user_says(root, GOAL);
    write(root, "context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));
    let before_points = hook_says(root, home.path(), "E o painel?");

    let items = report(&rt(root, &["grill", "--kinds", "feature", "--spec", SPEC, "--full"]))["points"]
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
    assert_eq!(unrouted_codes(&ended), [code_of(root, before_points)], "only the message that came with no open point is left loose: {ended}");

    let late = hook_says(root, home.path(), "E agora?");
    let log = store::read(&path).expect("a readable file").expect("the spec has its file");
    assert_eq!(log.get(late).and_then(|m| m.int("during")), None, "with every point closed nothing is carried");
}

/// A mensagem longa sem destino aparece no fim do levantamento como uma linha
/// com o código e o começo, só na resposta que fecha o último ponto: antes
/// dele e depois, na gravação seguinte, o relatório não traz `unrouted`, e o
/// texto inteiro sai pela leitura do item.
#[test]
fn the_end_of_the_survey_shows_each_unrouted_message_once_as_a_short_line() {
    const TAIL: &str = "FIM-DO-TEXTO-COLADO";
    let dir = repo();
    let home = tempfile::tempdir().expect("home");
    let root = dir.path();
    let opened = open(root, "feature", SPEC);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let said = hook_says(root, home.path(), GOAL);
    write(root, "context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));

    // O mesmo texto longo colado duas vezes, sem ponto aberto e sem registro
    // que o aponte; a fala curta que vem depois é a que a primeira resposta
    // aponta como origem.
    let pasted = format!("{}{TAIL}", "Linha colada do registro, com bastante detalhe sobre o merge.\n".repeat(60));
    let long = [hook_says(root, home.path(), &pasted), hook_says(root, home.path(), &pasted)];
    hook_says(root, home.path(), "Pode seguir.");

    let items = report(&rt(root, &["grill", "--kinds", "feature", "--spec", SPEC, "--full"]))["points"]
        .as_array()
        .cloned()
        .expect("the point list");
    let mut current = Value::Null;
    for item in &items {
        let facts = json!([{"text": "O merge começa no arquivo de entrada.", "source": "src/main.rs:2"}]);
        current = write(root, "point", &json!({"replaces": item["id"], "facts": facts}))["point"].clone();
    }
    let decision = json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": "O usuário respondeu ao ponto.",
        "keys": ["levantamento"], "why": "o usuário respondeu"})
    .to_string();
    let mut ended = None;
    for (i, _) in items.iter().enumerate() {
        let code = current["code"].as_str().expect("the point's code").to_string();
        let out = rt(root, &["answer", "--spec", SPEC, "--point", &code, "--type", "decision", "--json", &decision]);
        let answered = report(&out);
        assert_eq!(out.status.code(), Some(0), "answer {i}: {answered}");
        current = answered["point"].clone();
        if i + 1 < items.len() {
            assert!(answered.get("unrouted").is_none(), "an open point is left, so the end is not shown: {answered}");
        } else {
            ended = Some((answered, out.stdout.len(), String::from_utf8_lossy(&out.stdout).into_owned()));
        }
    }

    let (answered, size, raw) = ended.expect("the last answer");
    assert!(!raw.contains(TAIL), "the long text does not come back whole: {size} bytes");
    assert!(size < pasted.len(), "the answer that ends the survey is shorter than one pasted message: {size} bytes");
    let lines = unrouted_lines(&answered);
    let codes: Vec<String> = long.iter().map(|id| code_of(root, *id)).collect();
    assert_eq!(unrouted_codes(&answered), codes, "{answered}");
    for line in &lines {
        assert!(line.chars().count() < 200 && !line.contains('\n'), "one short line: {line}");
        assert!(line.contains("Linha colada do registro"), "the line carries the start of the text: {line}");
    }

    // A fala que chega depois do último ponto fica sem destino, e a decisão
    // seguinte a aponta: as duas colagens já foram mostradas, e a gravação
    // não as repete.
    hook_says(root, home.path(), "E o painel?");
    let out = rt(root, &["write", "decision", "--spec", SPEC, "--json", &decision]);
    let decided = report(&out);
    assert_eq!(out.status.code(), Some(0), "{decided}");
    assert!(decided.get("unrouted").is_none(), "the end of the survey is shown once: {decided}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains(TAIL), "{decided}");

    // O texto inteiro sai pelo código da linha.
    let full = rt(root, &["read", &format!("item-{}", codes[0]), "--spec", SPEC]);
    assert!(String::from_utf8_lossy(&full.stdout).contains(TAIL), "the item's reading has the whole message");
}

/// O `grill` que abre os pontos diz onde salvar a cópia do modelo visual
/// aprovado uma vez só no `hint`, depois de apresentar os pontos: diz que a
/// cópia vai logo depois de gravar o sim, e diz o caminho dela na pasta da
/// spec.
fn asks_for_the_design_copy_once(out: &Output, present: &str, after_yes: &str) {
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
    let opening = report(out);
    let hint = opening["hint"].as_str().unwrap_or_else(|| panic!("no hint: {opening}"));
    let design = format!("`.claude/spec/{SPEC}/desenho.html`");
    let shown = hint.find(present).unwrap_or_else(|| panic!("the grill presents the open points: {hint}"));
    let save = hint.find(&design).unwrap_or_else(|| panic!("the grill says where the approved design goes: {hint}"));
    assert!(shown < save && hint.contains(after_yes), "the copy goes right after the yes is recorded: {hint}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).matches("desenho.html").count(), 1, "{hint}");
}

/// O levantamento diz onde salvar a cópia do modelo visual aprovado quando
/// abre os pontos, no `grill` curto e no inteiro, para quem conduz salvá-la
/// logo depois de gravar o sim, enquanto o modelo ainda está à mão. Nenhuma
/// resposta depois repete o
/// pedido: nem as que gravam os fatos e as respostas, a do sim inclusive, a
/// que fecha o último ponto inclusive, nem o `grill` sem ponto aberto. A
/// escrita da cópia no caminho dito sai do gancho de escrita sem decisão
/// nenhuma, o mesmo gancho que, no mesmo projeto, barra o arquivo da spec; e
/// o despacho do revisor, pelo gancho, passa a apontar a cópia salva.
#[test]
fn the_survey_asks_for_the_approved_design_copy_when_it_opens_the_points_and_never_again() {
    const APPROVED_AT: usize = 2;
    let dir = repo();
    let home = tempfile::tempdir().expect("home");
    let root = dir.path();
    let design = format!(".claude/spec/{SPEC}/desenho.html");
    let mentions = |out: &Output| String::from_utf8_lossy(&out.stdout).matches("desenho.html").count();
    let opened = open(root, "feature", SPEC);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let said = hook_says(root, home.path(), GOAL);
    write(root, "context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));

    let short = rt(root, &["grill", "--kinds", "feature", "--spec", SPEC]);
    asks_for_the_design_copy_once(&short, "Os pontos abertos", "logo depois de gravar o sim");
    let grilled = rt(root, &["grill", "--kinds", "feature", "--spec", SPEC, "--full"]);
    asks_for_the_design_copy_once(&grilled, "Os pontos abertos", "logo depois de gravar o sim");
    let items = report(&grilled)["points"].as_array().cloned().expect("the point list");
    assert_eq!(items.len(), 9, "{items:?}");

    // Toda resposta depois do `grill` que abriu os pontos, contada.
    let mut later: Vec<(String, usize)> = Vec::new();
    let mut current = Value::Null;
    for item in &items {
        let facts = json!([{"text": "O merge começa no arquivo de entrada.", "source": "src/main.rs:2"}]);
        let out = rt(root, &["write", "point", "--spec", SPEC, "--json", &json!({"replaces": item["id"], "facts": facts}).to_string()]);
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
        later.push(("facts".to_string(), mentions(&out)));
        current = report(&out)["point"].clone();
    }
    let decision = |text: &str| {
        json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": text, "keys": ["levantamento"], "why": "o usuário respondeu"})
            .to_string()
    };
    let target = root.join(&design);
    let html = "<html><body><main>tela aprovada</main></body></html>\n";
    let mut ended = Value::Null;
    for i in 0..items.len() {
        let code = current["code"].as_str().expect("the point's code").to_string();
        let text = if i == APPROVED_AT { "O usuário aprovou o modelo da tela." } else { "O usuário respondeu ao ponto." };
        let out = rt(root, &["answer", "--spec", SPEC, "--point", &code, "--type", "decision", "--json", &decision(text)]);
        let answered = report(&out);
        assert_eq!(out.status.code(), Some(0), "answer {i}: {answered}");
        later.push((format!("answer {i}"), mentions(&out)));
        current = answered["point"].clone();
        ended = answered;
        if i != APPROVED_AT {
            continue;
        }
        // Gravado o sim, quem conduz salva a cópia onde o `grill` disse. O
        // gancho de escrita, que barra o arquivo da spec, deixa a cópia
        // passar sem decisão nenhuma.
        let writing = |path: &Path| {
            json!({"hook_event_name": "PreToolUse", "tool_name": "Write", "session_id": SESSION,
                "cwd": root.to_string_lossy(), "tool_input": {"file_path": path.to_string_lossy(), "content": html}})
        };
        let barred = on(root, home.path(), "PreToolUse", &writing(&target.with_file_name("spec.ndjson")));
        let barred: Value = serde_json::from_str(&barred).unwrap_or_else(|e| panic!("the gate answers JSON for the spec file ({e}): {barred}"));
        assert_eq!(barred["hookSpecificOutput"]["permissionDecision"], "deny", "the gate is live in this project: {barred}");
        let gate = on(root, home.path(), "PreToolUse", &writing(&target));
        assert!(gate.trim().is_empty(), "the copy is let through with no decision, warning or question: {gate}");
        std::fs::write(&target, html).expect("the conductor saves the copy");
    }
    assert!(ended["unrouted"].is_array(), "the last answer ends the survey: {ended}");
    let out = rt(root, &["grill", "--spec", SPEC, "--full"]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
    assert!(report(&out)["unrouted"].is_array(), "the grill with no open point: {}", String::from_utf8_lossy(&out.stdout));
    later.push(("grill at the end".to_string(), mentions(&out)));
    assert_eq!(later.len(), 9 + 9 + 1, "{later:?}");
    let repeated: Vec<&(String, usize)> = later.iter().filter(|(_, count)| *count > 0).collect();
    assert!(repeated.is_empty(), "no response after the grill repeats the copy: {repeated:?}");

    let prompt = format!("Revise o levantamento da spec {SPEC}: os pontos, as regras e as decisões.");
    let dispatch = json!({"hook_event_name": "PreToolUse", "tool_name": "Agent", "session_id": SESSION,
        "cwd": root.to_string_lossy(), "tool_input": {"subagent_type": "mustard-review", "description": "revisão", "prompt": prompt}});
    let answer: Value = serde_json::from_str(&on(root, home.path(), "PreToolUse", &dispatch)).expect("the hook answers JSON");
    let sent = answer["hookSpecificOutput"]["updatedInput"]["prompt"].as_str().unwrap_or_else(|| panic!("no rewritten prompt: {answer}"));
    assert!(sent.contains(&format!("{design}`")), "the outside reviewer is pointed to the saved copy: {sent}");
}

/// Num projeto em inglês, o `grill` curto que abre os pontos diz o mesmo, na
/// língua do projeto: onde salvar a cópia do modelo visual aprovado, uma vez
/// só no `hint`, logo depois de gravar o sim.
#[test]
fn the_short_grill_asks_for_the_design_copy_once_in_english() {
    let dir = repo();
    let home = tempfile::tempdir().expect("home");
    let root = dir.path();
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"en-US"},"git":{"flow":{"*":"dev","dev":"main"}}}"#)
        .expect("config");
    let opened = open(root, "feature", SPEC);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let said = hook_says(root, home.path(), GOAL);
    write(root, "context", &json!({"title": "Agree on the item", "agent": "- check by the test", "text": GOAL, "origin": said}));

    let short = rt(root, &["grill", "--kinds", "feature", "--spec", SPEC]);
    asks_for_the_design_copy_once(&short, "The open points", "right after recording the yes");
}

/// Duas tarefas que passam a depender uma da outra, em círculo, são
/// recusadas nomeando o círculo inteiro, na ordem, com os códigos das
/// tarefas; nada é gravado.
#[test]
fn cycle_between_tasks_is_refused_naming_the_cycle() {
    let dir = repo();
    let root = dir.path();
    open(root, "feature", SPEC);
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
    open(root, "feature", SPEC);
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
    let opened = open(root, "feature", SPEC);
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
