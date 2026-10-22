//! O passo de término de uma tarefa, gravado pelo binário de verdade, responde
//! ele mesmo a medida da conversa do agente de onda que o grava.
//!
//! O `run write step` lê a sessão e o agente pelo ambiente que o gancho do
//! terminal exporta (`MUSTARD_SESSION_ID`, `MUSTARD_AGENT_ID`), acha a conversa
//! principal da sessão na pasta de configuração do Claude Code e, ao lado
//! dela, a do agente; com a conversa acima do limite, a resposta traz a ordem
//! de entregar.

#[path = "support/binary.rs"]
mod binary;

use std::path::Path;
use std::process::Command;

use mustard_core::platform::i18n::Locale;
use serde_json::{Value, json};

#[path = "support/mod.rs"]
mod support;

const SPEC: &str = "medida";
const SESSION: &str = "s-medida";
const AGENT: &str = "a1";

/// O programa com o ambiente limpo do que vem de quem roda a suíte: nem a
/// sessão, nem o agente, nem a pasta do projeto da conversa de fora.
fn rt(root: &Path, args: &[&str]) -> Command {
    let mut cmd = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"));
    cmd.arg("run")
        .args(args)
        .arg("--root")
        .arg(root)
        .current_dir(root)
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("MUSTARD_AGENT_ID")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("MUSTARD_JEV_URL");
    cmd
}

fn stdout_json(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)))
}

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Um evento gravado pelo núcleo, como os comandos do fluxo o gravam.
fn seed(root: &Path, event_type: &str, fields: &Value) -> u64 {
    let path = mustard_core::io::spec_events::spec_file(root, SPEC).expect("spec file");
    std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
    let draft = fields.as_object().cloned().expect("an object");
    mustard_core::io::spec_events::write(&path, event_type, draft, &[]).expect(event_type).id
}

/// Um projeto no git, com a spec aprovada e a onda 1, de duas tarefas,
/// despachada pela rodada. Devolve o código de cada tarefa, na ordem.
fn dispatched_wave(root: &Path) -> Vec<String> {
    support::copies_leave_with_the_test(root);
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).expect("config");
    std::fs::write(root.join("a.rs"), "fn one() {}\n").expect("code");
    git(root, &["init", "-q", "-b", &format!("feature/{SPEC}")]);
    git(root, &["config", "user.email", "t@t"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    std::fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\n").expect("exclude");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "semente"]);

    seed(root, "state", &json!({"author": "binary", "phase": "plan", "branch": format!("feature/{SPEC}"), "base": "dev"}));
    let said = seed(root, "message", &json!({"author": "user", "text": "o plano"}));
    let crit = seed(root, "criterion", &json!({"when": "a", "then": "b", "proof": "git --version", "form": "ubiquitous", "origin": said}));
    seed(root, "wave", &json!({"author": "binary", "n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "x", "origin": said}));
    let task = seed(root, "task", &json!({"wave": 1, "text": "Tarefa 1.", "files": [{"path": "a.rs"}], "origin": said}));
    let decision = seed(root, "decision", &json!({"title": "O total em reais", "text": "O total fica em reais.", "agent": "- somar em centavos inteiros", "why": "o caixa fecha em reais", "keys": ["total"], "origin": said}));
    let second = seed(root, "task", &json!({"wave": 1, "text": "Tarefa 2: escrever o total.", "files": [{"path": "a.rs"}], "covers": [decision], "origin": said}));
    seed(root, "state", &json!({"author": "user", "phase": "approved", "witness": {"question": "Aprovar esta spec?", "answer": "Aprovar"}}));

    let dispatch = rt(root, &["round", "--dispatch", "--spec", SPEC]).output().expect("dispatch");
    assert!(dispatch.status.success(), "{}", String::from_utf8_lossy(&dispatch.stdout));
    let sent: Vec<u64> = stdout_json(&dispatch)["dispatch"].as_array().into_iter().flatten().filter_map(|e| e["wave"].as_u64()).collect();
    assert_eq!(sent, vec![1], "the round dispatched wave 1");

    let codes = spec_log(root).codes();
    [task, second].iter().map(|id| codes.get(id).cloned().expect("the task code")).collect()
}

/// A spec como está no disco.
fn spec_log(root: &Path) -> mustard_core::domain::spec_events::SpecLog {
    let path = mustard_core::io::spec_events::spec_file(root, SPEC).expect("spec file");
    mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file")
}

/// As tarefas que um recibo de leitura liberou, na ordem do arquivo.
fn released(root: &Path) -> Vec<String> {
    spec_log(root)
        .events
        .iter()
        .filter(|e| e.event_type == "call" && e.str_field("command") == Some("read"))
        .filter_map(|e| e.str_field("released").map(str::to_string))
        .collect()
}

/// Uma resposta do agente com o contexto somando `tokens`.
fn reply(tokens: u64) -> String {
    json!({
        "type": "assistant",
        "message": {"role": "assistant", "content": [{"type": "text", "text": "certo"}], "usage": {
            "input_tokens": 10, "cache_read_input_tokens": tokens - 2_010, "cache_creation_input_tokens": 2_000,
            "output_tokens": 300,
        }}
    })
    .to_string()
}

/// A pasta de configuração do Claude Code com a conversa principal da sessão
/// [`SESSION`], aberta em `root`, e a do agente [`AGENT`] ao lado dela, em
/// `subagents/`: o pedido da onda 1 e as respostas com o contexto de `sizes`.
fn conversations(config: &Path, root: &Path, sizes: &[u64]) {
    let project = config.join("projects").join("projeto-de-teste");
    std::fs::create_dir_all(&project).expect("projects folder");
    let opened = json!({"type": "user", "cwd": root.to_string_lossy(), "sessionId": SESSION,
        "message": {"role": "user", "content": "conduza a obra"}});
    std::fs::write(project.join(format!("{SESSION}.jsonl")), format!("{opened}\n")).expect("main conversation");

    let agents = project.join(SESSION).join("subagents");
    std::fs::create_dir_all(&agents).expect("subagents folder");
    let title = mustard_core::domain::wave_prompt::wave_title(SPEC, 1, Locale::PtBr);
    let first = json!({"type": "user", "message": {"role": "user", "content": format!("{title}\n\nO pedido.")}});
    let mut text = format!("{first}\n");
    for size in sizes {
        text.push_str(&reply(*size));
        text.push('\n');
    }
    std::fs::write(agents.join(format!("agent-{AGENT}.jsonl")), text).expect("agent conversation");
}

/// O passo de término da tarefa `item`, gravado como o agente de onda o
/// grava: com a sessão e o agente no ambiente e a pasta de configuração
/// `config`.
fn step(root: &Path, config: &Path, item: &str) -> Value {
    let body = json!({"wave": 1, "item": item, "text": "A tarefa ficou pronta."});
    let out = rt(root, &["write", "step", "--spec", SPEC, "--json", &body.to_string()])
        .env("CLAUDE_CONFIG_DIR", config)
        .env("MUSTARD_SESSION_ID", SESSION)
        .env("MUSTARD_AGENT_ID", AGENT)
        .output()
        .expect("run write step");
    assert!(out.status.success(), "write step: {}", String::from_utf8_lossy(&out.stdout));
    stdout_json(&out)
}

/// O passo de término da tarefa `item` sem a identidade do agente no
/// ambiente: a medida não sai.
fn step_without_measure(root: &Path, item: &str) -> Value {
    let body = json!({"wave": 1, "item": item, "text": "A tarefa ficou pronta."});
    let out = rt(root, &["write", "step", "--spec", SPEC, "--json", &body.to_string()]).output().expect("run write step");
    assert!(out.status.success(), "write step: {}", String::from_utf8_lossy(&out.stdout));
    stdout_json(&out)
}

/// A conversa do agente passou do limite de 200 mil tokens: o passo que
/// termina a tarefa responde, no campo `size`, a ordem de entregar.
#[test]
fn the_task_end_step_answers_the_order_to_deliver_above_the_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("projeto");
    std::fs::create_dir_all(&root).expect("root");
    let config = dir.path().join("claude-config");
    let tasks = dispatched_wave(&root);
    conversations(&config, &root, &[30_000, 210_000]);

    let report = step(&root, &config, &tasks[0]);
    assert_eq!(report["ok"], json!(true), "{report}");
    let size = report["size"].as_str().unwrap_or_else(|| panic!("the step answers the measure: {report}"));
    assert!(size.contains("Grave a entrega"), "the answer orders the delivery: {size}");
    assert!(size.contains("210"), "the answer carries the size of the conversation: {size}");
    assert!(report.get("next_task").is_none(), "above the limit no task comes next: {report}");
    assert!(released(&root).is_empty(), "nothing was released: {report}");
}

/// Abaixo do limite, o passo que termina a primeira tarefa traz na resposta
/// o bloco inteiro da seguinte — o texto dela e o do que ela atende — e a
/// libera; sem a medida, também.
#[test]
fn below_the_limit_the_task_end_step_brings_the_next_whole_task_and_releases_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("projeto");
    std::fs::create_dir_all(&root).expect("root");
    let config = dir.path().join("claude-config");
    let tasks = dispatched_wave(&root);
    conversations(&config, &root, &[30_000, 60_000]);

    let report = step(&root, &config, &tasks[0]);
    assert!(report["size"].as_str().is_some_and(|size| size.contains("Siga")), "the answer says to go on: {report}");
    let next = report["next_task"].as_str().unwrap_or_else(|| panic!("the step brings the next task: {report}"));
    for whole in [tasks[1].as_str(), "Tarefa 2: escrever o total.", "- somar em centavos inteiros"] {
        assert!(next.contains(whole), "{whole}: {next}");
    }
    assert_eq!(released(&root), [tasks[1].clone()]);

    let again = step_without_measure(&root, &tasks[0]);
    assert!(again.get("size").is_none(), "{again}");
    assert!(again["next_task"].as_str().is_some_and(|text| text.contains("Tarefa 2: escrever o total.")), "without a measure it goes on: {again}");
}

/// O passo que termina a última tarefa diz que todas terminaram e manda
/// entregar, sem próxima tarefa; o passo de um critério não libera nada.
#[test]
fn the_last_task_end_step_says_every_task_finished() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("projeto");
    std::fs::create_dir_all(&root).expect("root");
    let tasks = dispatched_wave(&root);
    let criterion = spec_log(&root).codes().values().find(|code| code.contains("-CRIT-")).cloned().expect("the criterion");

    let proved = step_without_measure(&root, &criterion);
    assert!(proved.get("next_task").is_none() && proved.get("tasks_finished").is_none(), "{proved}");
    assert!(released(&root).is_empty(), "{proved}");
    step_without_measure(&root, &tasks[0]);
    let last = step_without_measure(&root, &tasks[1]);
    assert!(last.get("next_task").is_none(), "{last}");
    let finished = last["tasks_finished"].as_str().unwrap_or_else(|| panic!("the last step says every task finished: {last}"));
    assert!(finished.contains("Grave a entrega"), "{finished}");
}

/// O que o gancho de depois da ferramenta, rodado em `root` com o JSON
/// `payload` na entrada, injeta na conversa; vazio quando nada.
fn after_tool(root: &Path, payload: &Value) -> String {
    use std::io::Write;
    let mut child = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["on", "PostToolUse"])
        .current_dir(root)
        .env_remove("MUSTARD_AGENT_ID")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the hook runs");
    child.stdin.take().expect("stdin").write_all(payload.to_string().as_bytes()).expect("the payload");
    let out = child.wait_with_output().expect("the hook ends");
    assert_eq!(out.status.code(), Some(0), "a hook always exits 0");
    let text = String::from_utf8_lossy(&out.stdout);
    let answer: Value = serde_json::from_str(text.trim()).unwrap_or(Value::Null);
    answer["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_default().to_string()
}

/// O passo de término gravado sem a identidade do agente no ambiente não
/// traz a medida, e quem mede é o gancho de depois da ferramenta. Com o
/// `--json` lido de um arquivo da pasta temporária, relativo ao `cd` da
/// chamada, o gancho relê o arquivo, acha a tarefa no `item` e mede a
/// conversa do agente de onda.
#[test]
fn the_hook_measures_a_task_end_step_whose_json_came_from_a_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("projeto");
    std::fs::create_dir_all(&root).expect("root");
    let config = dir.path().join("claude-config");
    let tasks = dispatched_wave(&root);
    conversations(&config, &root, &[30_000, 60_000]);
    let scratch = dir.path().join("temporario");
    std::fs::create_dir_all(&scratch).expect("the temporary folder");
    let body = json!({"wave": 1, "item": tasks[0], "text": "A tarefa ficou pronta: \"saudação\" (nova)."});
    std::fs::write(scratch.join("passo.json"), body.to_string()).expect("the step file");

    let args = ["write", "step", "--spec", SPEC, "--json", "@passo.json"];
    let out = rt(&root, &args).current_dir(&scratch).output().expect("run write step");
    let report = stdout_json(&out);
    assert!(out.status.success() && report.get("size").is_none(), "the step itself does not measure: {report}");

    let command = format!("cd {} && mustard-rt run {} --root {}", scratch.display(), args.join(" "), root.display());
    let main = config.join("projects").join("projeto-de-teste").join(format!("{SESSION}.jsonl"));
    let context = after_tool(&root, &json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "tool_response": {"stdout": String::from_utf8_lossy(&out.stdout), "stderr": ""},
        "session_id": SESSION,
        "agent_id": AGENT,
        "transcript_path": main.to_string_lossy(),
        "cwd": root.to_string_lossy(),
    }));
    assert!(context.contains("Siga para a próxima tarefa.") && context.contains("60 mil"), "the hook measures the task end: {context}");
}
