//! `mustard-rt run open` pelo binário: um passo que pergunta de volta sai com
//! exit 0, a recusa com exit 1, e a abertura cria a branch e a spec com o
//! nome exato.

#[path = "support/binary.rs"]
mod binary;
use crate::one_wave_spec;

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?} failed");
}

/// Um repositório com `main` e `dev`, as bases declaradas e código em Rust e
/// em TypeScript, parado em `dev`. O Mustard fica fora do git.
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
    std::fs::write(root.join("src").join("main.rs"), "fn main() {}\n").expect("code");
    std::fs::create_dir_all(root.join("web")).expect("web");
    std::fs::write(root.join("web").join("app.ts"), "export const merge = () => true;\n").expect("typescript code");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "init"]);
    git(root, &["checkout", "-q", "-b", "dev"]);
    dir
}

fn open(root: &Path, args: &[&str]) -> Output {
    binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .arg("run")
        .arg("open")
        .args(args)
        .arg("--root")
        .arg(root)
        .current_dir(root)
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .expect("run open")
}

fn report(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).expect("one JSON report")
}

#[test]
fn open_answers_a_step_with_exit_0_a_refusal_with_exit_1_and_opens_the_spec() {
    let dir = repo();
    let root = dir.path();

    let step = open(root, &["--kind", "feature", "--name", "trava de pendências", "--base", "dev"]);
    assert_eq!(step.status.code(), Some(0));
    assert_eq!(report(&step)["step"], "confirm_name");

    let refused = open(root, &["--kind", "feature", "--name", "x", "--base", "nao-existe"]);
    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(report(&refused)["reason"], "base-not-found");

    let opened = open(root, &["--name", "feature/trava-de-pendencias", "--base", "dev"]);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let opened = report(&opened);
    assert_eq!(opened["branch"], "feature/trava-de-pendencias");
    assert_eq!(opened["step"], "ask_goal");
    assert!(root.join(".claude").join("spec").join("trava-de-pendencias").join("spec.ndjson").is_file());
}

/// A conferência do lint na base sai da abertura e roda uma vez, no primeiro
/// despacho de onda da spec: a abertura não roda o lint, a primeira rodada que
/// solta a onda roda e grava a base, e a rodada seguinte, que assume a volta,
/// não roda de novo. Em campo, a abertura gastava 27 segundos no lint, que só
/// o fechamento lê.
#[test]
fn the_base_lint_runs_once_at_the_first_wave_dispatch_and_never_at_the_opening() {
    let project = one_wave_spec::Project::new("conferencia", &json!({"lintCommand": "echo ran >> lint-runs.log"}), &["lint-runs.log"]);
    let runs = || std::fs::read_to_string(project.root.join("lint-runs.log")).unwrap_or_default().lines().count();
    let base_marks = || project.log().events.iter().filter(|e| e.event_type == "stage_run" && e.str_field("phase") == Some("base")).count();

    project.approved();
    assert_eq!((runs(), base_marks()), (0, 0), "the opening does not run the lint");

    let first = project.run(&["round", "--dispatch", "--spec", "conferencia"]);
    assert_eq!(first["dispatch"][0]["wave"], json!(1), "{first}");
    assert_eq!((runs(), base_marks()), (1, 1), "the first wave dispatch runs the lint and records the base: {first}");

    let second = project.deliver_the_first_wave();
    assert_eq!((runs(), base_marks()), (1, 1), "the next round does not run it again: {second}");
}

/// A rodada pedida sem `--spec` acha a spec atual, como a rodada a acha, e o
/// primeiro despacho dela roda o lint na base e grava a marca do mesmo jeito
/// que a rodada com `--spec`. A rodada só despacha na branch da spec.
#[test]
fn the_round_without_spec_runs_the_base_lint_for_the_current_spec() {
    let project = one_wave_spec::Project::new("sem-spec", &json!({"lintCommand": "echo ran >> lint-runs.log"}), &["lint-runs.log"]);
    let runs = || std::fs::read_to_string(project.root.join("lint-runs.log")).unwrap_or_default().lines().count();
    let base_marks = || project.log().events.iter().filter(|e| e.event_type == "stage_run" && e.str_field("phase") == Some("base")).count();

    project.approved();
    let first = project.run(&["round", "--dispatch"]);
    assert_eq!(first["dispatch"][0]["wave"], json!(1), "{first}");
    assert_eq!((runs(), base_marks()), (1, 1), "the dispatch without --spec runs the lint and records the base: {first}");
}

/// A conversa da sessão `session`, aberta na pasta `project`, como o Claude
/// Code a guarda dentro da pasta de configuração `config`: na pasta com o
/// caminho do projeto em hífens, uma linha JSON por mensagem, cada uma com o
/// `cwd` de onde a conversa roda.
fn conversation(config: &Path, project: &Path, session: &str, lines: &[Value]) {
    let name: String = project.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let folder = config.join("projects").join(name);
    std::fs::create_dir_all(&folder).expect("projects folder");
    let text: Vec<String> = lines
        .iter()
        .map(|line| {
            let mut line = line.clone();
            if let Some(fields) = line.as_object_mut() {
                fields.insert("cwd".into(), Value::from(project.to_string_lossy().into_owned()));
            }
            line.to_string()
        })
        .collect();
    std::fs::write(folder.join(format!("{session}.jsonl")), text.join("\n") + "\n").expect("conversation");
}

fn user_line(content: Value, extra: Value) -> Value {
    let mut line = serde_json::json!({ "type": "user", "message": { "role": "user", "content": content } });
    if let (Some(line), Some(extra)) = (line.as_object_mut(), extra.as_object()) {
        line.extend(extra.clone());
    }
    line
}

#[test]
fn opening_a_spec_records_the_messages_the_user_typed_before_in_order() {
    let dir = repo();
    let root = dir.path();
    let config = tempfile::tempdir().expect("config dir");
    let session = "6b0e4c1a-2f3d-4e5f-8a9b-0c1d2e3f4a5b";
    let asked = "Quero que o merge fique travado enquanto houver pendência aberta.";
    let opening = "Pode abrir a spec para isso.";
    let human = serde_json::json!({ "origin": { "kind": "human" } });
    conversation(
        config.path(),
        root,
        session,
        &[
            user_line(serde_json::json!(asked), human.clone()),
            serde_json::json!({ "type": "assistant", "message": { "role": "assistant", "content": [
                { "type": "text", "text": "Vou ler o código do merge." },
                { "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "ls" } }
            ] } }),
            user_line(
                serde_json::json!([{ "type": "tool_result", "tool_use_id": "t1", "content": "src" }]),
                serde_json::json!({}),
            ),
            user_line(
                serde_json::json!("<local-command-caveat>Caveat: the messages below were generated by the user while running local commands.</local-command-caveat>"),
                serde_json::json!({ "isMeta": true }),
            ),
            user_line(
                serde_json::json!("<task-notification>\n<status>completed</status>\n</task-notification>"),
                serde_json::json!({ "origin": { "kind": "task-notification" } }),
            ),
            user_line(serde_json::json!([{ "type": "text", "text": opening }]), human),
        ],
    );

    let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "open", "--name", "feature/trava-de-pendencias", "--base", "dev", "--root"])
        .arg(root)
        .current_dir(root)
        .env("CLAUDE_CONFIG_DIR", config.path())
        .env("MUSTARD_SESSION_ID", session)
        .output()
        .expect("run open");
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
    let opened = report(&out);

    let log = std::fs::read_to_string(root.join(".claude").join("spec").join("trava-de-pendencias").join("spec.ndjson"))
        .expect("the spec was born");
    let users: Vec<Value> = log
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event["type"] == "message" && event["author"] == "user")
        .collect();
    let texts: Vec<&str> = users.iter().filter_map(|event| event["text"].as_str()).collect();
    assert_eq!(texts, vec![asked, opening], "only what the user typed, in order");
    let ids: Vec<Value> = users.iter().map(|event| event["id"].clone()).collect();
    assert_eq!(opened["earlier_messages"], Value::Array(ids.clone()), "{opened}");
    assert!(opened["hint"].as_str().is_some_and(|hint| hint.contains("`earlier_messages`")), "{opened}");

    let goal = serde_json::json!({
        "title": "O objetivo da obra",
        "agent": "- conferir o objetivo",
        "text": "Travar o merge enquanto houver pendência aberta.",
        "origin": ids[0],
    });
    let written = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "write", "context", "--spec", "trava-de-pendencias", "--json", &goal.to_string(), "--root"])
        .arg(root)
        .current_dir(root)
        .env("CLAUDE_CONFIG_DIR", config.path())
        .env("MUSTARD_SESSION_ID", session)
        .output()
        .expect("run write");
    assert_eq!(report(&written)["ok"], true, "{}", String::from_utf8_lossy(&written.stdout));
}

#[test]
fn opening_without_the_session_conversation_warns_and_still_opens() {
    let dir = repo();
    let root = dir.path();
    let config = tempfile::tempdir().expect("config dir");
    let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "open", "--name", "feature/sem-conversa", "--base", "dev", "--root"])
        .arg(root)
        .current_dir(root)
        .env("CLAUDE_CONFIG_DIR", config.path())
        .env("MUSTARD_SESSION_ID", "sessao-sem-arquivo")
        .output()
        .expect("run open");
    let opened = report(&out);
    assert_eq!(opened["step"], "ask_goal", "{opened}");
    assert!(opened["earlier_messages_missing"].as_str().is_some_and(|note| !note.is_empty()), "{opened}");
    assert!(opened.get("earlier_messages").is_none(), "{opened}");
    // The hint never says the earlier messages are in the spec when none came.
    let hint = opened["hint"].as_str().unwrap_or_default();
    assert!(!hint.contains("earlier_messages") && !hint.contains("antes da abertura"), "{opened}");
}

/// A sessão que o Claude Code põe no ambiente é a de uma conversa recente de
/// outro projeto, guardada na pasta pessoal: a abertura neste projeto não traz
/// fala nenhuma dela e avisa que as mensagens de antes ficaram de fora.
#[test]
fn opening_never_brings_the_messages_of_a_conversation_from_another_project() {
    let dir = repo();
    let root = dir.path();
    let other = repo();
    let home = tempfile::tempdir().expect("home");
    let session = "0f1e2d3c-4b5a-4968-8776-655443322110";
    let human = serde_json::json!({ "origin": { "kind": "human" } });
    conversation(
        &home.path().join(".claude"),
        other.path(),
        session,
        &[
            user_line(serde_json::json!("Analise o fluxo de pagamentos do outro projeto."), human.clone()),
            user_line(serde_json::json!([{ "type": "text", "text": "Nesta conversa, nada de abrir spec." }]), human),
        ],
    );

    let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "open", "--name", "feature/trava-de-pendencias", "--base", "dev", "--root"])
        .arg(root)
        .current_dir(root)
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("CLAUDE_CODE_SESSION_ID", session)
        .output()
        .expect("run open");
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
    let opened = report(&out);

    let log = std::fs::read_to_string(root.join(".claude").join("spec").join("trava-de-pendencias").join("spec.ndjson"))
        .expect("the spec was born");
    let users: Vec<Value> = log
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event["type"] == "message" && event["author"] == "user")
        .collect();
    assert_eq!(users, Vec::<Value>::new(), "no message of the other project's conversation: {opened}");
    assert!(opened.get("earlier_messages").is_none(), "{opened}");
    assert!(opened["earlier_messages_missing"].as_str().is_some_and(|note| !note.is_empty()), "{opened}");
}

/// As falas do usuário que a spec guarda, em ordem.
fn user_texts(root: &Path, spec: &str) -> Vec<String> {
    let log = std::fs::read_to_string(root.join(".claude").join("spec").join(spec).join("spec.ndjson")).expect("the spec was born");
    log.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event["type"] == "message" && event["author"] == "user")
        .filter_map(|event| event["text"].as_str().map(str::to_string))
        .collect()
}

/// O Claude Code repete a fala digitada numa linha `api-request-blob`, e põe
/// o lembrete do sistema dentro da fala. A abertura grava a fala uma vez, sem
/// o lembrete; e a fala seguinte, que chega pelo gancho com o JSON do Claude
/// Code, também entra sem ele, pelo mesmo filtro.
#[test]
fn the_typed_message_enters_once_and_without_the_system_text() {
    let dir = repo();
    let root = dir.path();
    let config = tempfile::tempdir().expect("config dir");
    let session = "3c9d1e7f-5a2b-4c6d-9e8f-1a2b3c4d5e6f";
    let asked = "Quero que a leitura da spec fique menor.";
    let human = json!({ "origin": { "kind": "human" } });
    conversation(
        config.path(),
        root,
        session,
        &[
            user_line(json!(asked), human.clone()),
            json!({ "type": "api-request-blob", "sessionId": session, "hash": "0".repeat(64),
                "message": { "role": "user", "content": [{ "type": "text", "text": asked }] } }),
            user_line(json!("Pode abrir a spec.\n\n<system-reminder>texto do sistema na abertura</system-reminder>"), human),
        ],
    );
    let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "open", "--name", "feature/leitura-menor", "--base", "dev", "--root"])
        .arg(root)
        .current_dir(root)
        .env("CLAUDE_CONFIG_DIR", config.path())
        .env("MUSTARD_SESSION_ID", session)
        .output()
        .expect("run open");
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
    assert_eq!(user_texts(root, "leitura-menor"), vec![asked, "Pode abrir a spec."], "{}", String::from_utf8_lossy(&out.stdout));

    let mut hook = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["on", "UserPromptSubmit"])
        .current_dir(root)
        .env("CLAUDE_CONFIG_DIR", config.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the hook runs");
    let payload = json!({ "hook_event_name": "UserPromptSubmit", "session_id": session, "cwd": root.to_string_lossy(),
        "prompt": "E o painel também.\n\n<system-reminder>texto do sistema no gancho</system-reminder>" });
    std::io::Write::write_all(&mut hook.stdin.take().expect("stdin"), payload.to_string().as_bytes()).expect("payload");
    assert_eq!(hook.wait_with_output().expect("the hook finishes").status.code(), Some(0));
    assert_eq!(user_texts(root, "leitura-menor"), vec![asked, "Pode abrir a spec.", "E o painel também."]);
}
