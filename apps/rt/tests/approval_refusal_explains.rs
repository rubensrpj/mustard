// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A testemunha da aprovação diz por que nada foi gravado.
//!
//! A testemunha grava o estado aprovado só quando o usuário escolhe uma das
//! opções oferecidas que começa por "Aprovar" ("Approve"). Uma opção como
//! "Sim, pode ir" não aprova, e uma resposta digitada, que não é nenhuma das
//! opções, também não. Nos dois casos, quem fez a pergunta precisa saber o
//! motivo na hora, e o texto de um gancho no stderr não chega ao assistente:
//! a testemunha responde no contexto do `PostToolUse`.
//!
//! Roda `mustard-rt on PostToolUse` como processo, porque os ganchos são
//! privados da biblioteca, e confere o `state` da spec no `spec.ndjson`.
//!
//! No programa único de integração do pacote, o nome completo do teste leva o
//! do módulo: `approval_refusal_explains::approval_refusal_names_the_unmet_condition`.
//! Um critério que rode o teste com `--exact` cita esse caminho inteiro.
//!
//! O clique que confirma o objetivo passa pelo mesmo caminho de uma sessão:
//! a spec aberta pelo `run open`, a fala do usuário pelo gancho da entrada, o
//! objetivo pelo `run write`, a fala do assistente na conversa do Claude Code
//! e o clique pelo `PostToolUse`. A testemunha diz na hora se o clique valeu,
//! e cada autor fica com o que disse: o usuário com a opção, o assistente com
//! a pergunta.

#[path = "support/binary.rs"]
mod binary;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::io::spec_events as store;
use serde_json::{json, Value};

const QUESTION: &str = "Aprovar esta spec?";

/// Responde a pergunta com as opções `offered` e a resposta `answers`, na
/// pasta `cwd`, e devolve o que a testemunha disse ao assistente, ou vazio.
fn answer(cwd: &Path, session: &str, offered: &[&str], answers: Value) -> String {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let options: Vec<Value> = offered.iter().map(|l| json!({ "label": l })).collect();
    let input = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "AskUserQuestion",
        "tool_input": { "questions": [{ "question": QUESTION, "options": options }] },
        "tool_response": { "questions": [], "answers": answers },
        "session_id": session,
        "cwd": cwd.to_str().unwrap()
    });
    let mut child = binary::isolated(bin)
        .args(["on", "PostToolUse"])
        .current_dir(cwd)
        .env("CLAUDE_PROJECT_DIR", cwd)
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mustard-rt");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = write!(stdin, "{input}");
    }
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0), "mustard-rt PostToolUse must exit 0 (fail-open)");
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str::<Value>(stdout.trim())
        .ok()
        .and_then(|v| v["hookSpecificOutput"]["additionalContext"].as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Uma spec `epic` na fase de plano, com uma tarefa e o plano passado, ligada
/// à sessão, num projeto em pt-BR.
fn spec_in_plan(project: &Path, session: &str) {
    fs::write(project.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
    let path = store::spec_file(project, "epic").unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    // A aprovação pede uma tarefa e o plano passado depois dela.
    let task = json!({ "text": "Somar dois números.", "author": "binary" });
    store::write(&path, "task", task.as_object().cloned().unwrap(), &[]).unwrap();
    let plan = json!({ "phase": "plan" });
    store::write(&path, "state", plan.as_object().cloned().unwrap(), &[]).unwrap();
    let session_dir = project.join(".claude").join(".session").join(session);
    fs::create_dir_all(&session_dir).unwrap();
    fs::write(session_dir.join("active-spec"), "epic").unwrap();
}

/// A fase atual da spec `epic`, lida do último `state` do arquivo.
fn phase(project: &Path) -> String {
    let body = fs::read_to_string(store::spec_file(project, "epic").unwrap()).unwrap();
    body.lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["type"] == "state")
        .and_then(|v| v["phase"].as_str().map(str::to_string))
        .unwrap_or_default()
}

#[test]
fn approval_refusal_names_the_unmet_condition() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    spec_in_plan(project, "s-decline");

    // Uma escolha de verdade, com palavras que não começam por "Aprovar".
    let said = answer(
        project,
        "s-decline",
        &["Sim, pode ir", "Não, revisar"],
        json!({ QUESTION: "Sim, pode ir" }),
    );

    assert!(said.contains("epic"), "names the spec awaiting approval:\n{said}");
    assert!(said.contains("Sim, pode ir"), "quotes the option that failed:\n{said}");
    assert!(said.contains("\"Aprovar\""), "names the word that would approve:\n{said}");
    assert_eq!(phase(project), "plan", "explaining the decline records nothing");
}

#[test]
fn a_recognised_approval_records_the_state_and_suggests_clear() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    spec_in_plan(project, "s-ok");

    let said = answer(project, "s-ok", &["Aprovar", "Ajustar"], json!({ QUESTION: "Aprovar" }));

    assert_eq!(phase(project), "approved", "a recognised approval records the state");
    assert!(said.contains("/clear"), "the witness suggests clearing the window:\n{said}");
    assert!(!said.contains("nada foi gravado"), "nothing was declined:\n{said}");
}

/// A resposta digitada em vez de escolhida, com uma palavra de aprovação no
/// meio, não aprova, e quem perguntou fica sabendo que é preciso escolher a
/// opção.
#[test]
fn free_text_is_declined_and_told_to_pick_instead() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    spec_in_plan(project, "s-typed");

    let said = answer(
        project,
        "s-typed",
        &["Aprovar", "Ajustar"],
        json!({ QUESTION: "o relato diz que ninguém conseguia aprovar a spec" }),
    );

    assert_eq!(phase(project), "plan", "free text never approves:\n{said}");
    assert!(
        said.contains("Texto livre nunca aprova") && said.contains("escolhendo a opção"),
        "the operator is told to pick the option instead of typing:\n{said}"
    );
}

#[test]
fn a_dismissed_dialog_explains_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    spec_in_plan(project, "s-cancel");

    // Uma pergunta cancelada não respondeu nada, e não há o que explicar.
    let said = answer(project, "s-cancel", &["Aprovar"], json!({}));

    assert!(said.is_empty(), "a dismissed dialog is not a failed condition:\n{said}");
    assert_eq!(phase(project), "plan");
}

// ---------------------------------------------------------------------------
// O clique do objetivo
// ---------------------------------------------------------------------------

const GOAL_SPEC: &str = "objetivo";
const GOAL_SESSION: &str = "s-objetivo";
const GOAL: &str = "Reduzir o custo da leitura do mapa.";
const SHORT: &str = "Confirma?";

/// Um projeto em pt-BR, num repositório parado em `dev`, com a spec
/// `objetivo` aberta pelo `run open`, a fala do usuário gravada pelo gancho
/// da entrada e o objetivo [`GOAL`] gravado pelo `run write`, com a origem
/// nessa fala, que não começa por ele: só o clique confirma.
struct GoalProject {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl GoalProject {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let (root, home) = (base.join("projeto"), base.join("casa"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&home).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git").args(args).current_dir(&root).output().expect("git runs");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        git(&["checkout", "-q", "-b", "main"]);
        fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\n").unwrap();
        fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"},"git":{"flow":{"*":"dev","dev":"main"}}}"#)
            .unwrap();
        fs::write(root.join("README.md"), "oi\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "init"]);
        git(&["checkout", "-q", "-b", "dev"]);
        let project = Self { _dir: dir, root, home };
        let opened = project.run(&["open", "--kind", "feature", "--name", GOAL_SPEC, "--base", "dev"]);
        assert_eq!(opened["ok"], json!(true), "{opened}");
        project.hook(
            "UserPromptSubmit",
            &json!({"hook_event_name": "UserPromptSubmit", "prompt": "quero que a leitura custe menos",
                "session_id": GOAL_SESSION, "cwd": project.root.to_string_lossy()}),
        );
        let said = project.last("message").expect("the entry hook recorded the message")["id"].clone();
        let goal = json!({"title": "Leitura", "agent": "- conferir a leitura", "text": GOAL, "origin": said});
        let written = project.run(&["write", "context", "--spec", GOAL_SPEC, "--json", &goal.to_string()]);
        assert_eq!(written["ok"], json!(true), "{written}");
        project
    }

    /// O binário com `args`, no projeto, com a pasta pessoal falsa e nenhuma
    /// sessão nem spec forçada.
    fn command(&self, args: &[&str], stdin: &str) -> String {
        let mut child = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CLAUDE_PROJECT_DIR", &self.root)
            .env("MUSTARD_CLAUDE_BIN", self.home.join("sem-claude"))
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(stdin.as_bytes());
        }
        let out = child.wait_with_output().expect("the binary finishes");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn run(&self, args: &[&str]) -> Value {
        let mut all = vec!["run"];
        all.extend_from_slice(args);
        let text = self.command(&all, "");
        serde_json::from_str(text.trim()).unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}"))
    }

    /// O que o gancho `event` disse ao assistente, ou vazio.
    fn hook(&self, event: &str, payload: &Value) -> String {
        let out = self.command(&["on", event], &payload.to_string());
        serde_json::from_str::<Value>(out.trim())
            .ok()
            .and_then(|v| v["hookSpecificOutput"]["additionalContext"].as_str().map(str::to_string))
            .unwrap_or_default()
    }

    /// O assistente diz `shown` e, na mesma vez, faz a pergunta `question`,
    /// com o cabeçalho `header` e as opções `options`; o usuário responde
    /// `answer`. Devolve o que a testemunha disse ao assistente.
    fn ask(&self, shown: &str, question: &str, header: &str, options: &[&str], answer: &str) -> String {
        let transcript = self.home.join("conversa.jsonl");
        let lines = [
            json!({"type": "user", "message": {"role": "user", "content": "quero que a leitura custe menos"}}),
            json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": shown}]}}),
            json!({"type": "assistant", "message": {"role": "assistant", "content": [
                {"type": "tool_use", "id": "t1", "name": "AskUserQuestion", "input": {}}
            ]}}),
        ];
        fs::write(&transcript, lines.map(|line| line.to_string()).join("\n") + "\n").unwrap();
        let options: Vec<Value> = options.iter().map(|label| json!({ "label": label })).collect();
        self.hook(
            "PostToolUse",
            &json!({
                "hook_event_name": "PostToolUse",
                "tool_name": "AskUserQuestion",
                "tool_input": {"questions": [{"question": question, "header": header, "options": options}]},
                "tool_response": {"questions": [], "answers": {question: answer}},
                "session_id": GOAL_SESSION,
                "cwd": self.root.to_string_lossy(),
                "transcript_path": transcript.to_string_lossy(),
            }),
        )
    }

    /// A pergunta curta do objetivo, depois da fala `shown`, respondida com
    /// "Confirmar objetivo".
    fn confirm_after(&self, shown: &str) -> String {
        self.ask(shown, SHORT, "MustardGoal", &["Confirmar objetivo", "Ajustar objetivo"], "Confirmar objetivo")
    }

    /// Os eventos da spec, como estão no arquivo.
    fn events(&self) -> Vec<Value> {
        let file = store::spec_file(&self.root, GOAL_SPEC).unwrap();
        fs::read_to_string(file).unwrap().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
    }

    /// O último evento do tipo `event_type`.
    fn last(&self, event_type: &str) -> Option<Value> {
        self.events().into_iter().rfind(|event| event["type"] == event_type)
    }

    /// A confirmação do objetivo como a retomada a lê.
    fn goal_approval(&self) -> Value {
        self.run(&["resume", "--spec", GOAL_SPEC])["goal_approval"].clone()
    }
}

/// O objetivo mostrado numa fala e depois a pergunta curta: o clique confirma
/// e a testemunha diz isso na hora. A mensagem do usuário tem só a opção que
/// ele clicou, a pergunta fica no clique, como do assistente, e a fala do
/// objetivo é a última resposta antes do clique — a pergunta não vira
/// resposta no meio.
#[test]
fn a_short_question_after_the_goal_confirms_and_keeps_each_author() {
    let project = GoalProject::new();
    let shown = format!("O objetivo é: {GOAL}");

    let said = project.confirm_after(&shown);

    assert!(said.contains("Objetivo confirmado"), "the witness says the click counted:\n{said}");
    assert_eq!(project.goal_approval()["confirmed"], json!(true));
    let events = project.events();
    let click = events.iter().rposition(|e| e["type"] == "message").expect("the click");
    assert_eq!(events[click]["author"], json!("user"));
    assert_eq!(events[click]["text"], json!("Confirmar objetivo"), "the user said only the option");
    assert_eq!(events[click]["witness"]["question"], json!(SHORT), "the question stays in the click");
    let goal = events.iter().position(|e| e["type"] == "context").expect("the goal");
    let responses: Vec<&Value> = events[goal..click].iter().filter(|e| e["type"] == "response").collect();
    assert_eq!(responses.len(), 1, "only the speech goes between the goal and the click: {responses:?}");
    assert_eq!(responses[0]["text"], json!(shown));
}

/// O objetivo em negrito, com crase e uma vírgula a mais na fala é o mesmo
/// objetivo: o clique confirma.
#[test]
fn a_goal_shown_in_bold_with_backticks_and_an_extra_comma_still_confirms() {
    let project = GoalProject::new();

    let said = project.confirm_after("O objetivo é: **Reduzir o custo, da `leitura` do mapa**.");

    assert!(said.contains("Objetivo confirmado"), "{said}");
    assert_eq!(project.goal_approval()["confirmed"], json!(true));
}

/// Sem o objetivo na fala antes da pergunta curta, o clique não vale, e a
/// testemunha diz isso no próprio clique, com o objetivo como está gravado —
/// sem esperar o comando seguinte.
#[test]
fn a_click_without_the_goal_in_sight_is_refused_on_the_spot() {
    let project = GoalProject::new();

    let said = project.confirm_after("Vou confirmar com você.");

    assert!(said.contains("o clique não valeu"), "the refusal is said at the click:\n{said}");
    assert!(said.contains(GOAL), "the recorded goal goes with it:\n{said}");
    assert!(said.contains("MustardGoal"), "and how to ask again:\n{said}");
    let approval = project.goal_approval();
    assert_eq!(approval["confirmed"], json!(false), "{approval}");
    assert_eq!(approval["reason"], json!("goal-text-differs"), "{approval}");
}

/// A pergunta comum, sem testemunha, também separa os autores: a mensagem do
/// usuário tem só a opção clicada, ou só o que ele digitou em "Outro", e a
/// pergunta fica na própria mensagem, como do assistente. Nenhuma resposta
/// nasce dela, e a testemunha não diz nada.
#[test]
fn a_common_question_keeps_only_the_answer_with_the_user() {
    let project = GoalProject::new();
    let before = project.events().iter().filter(|e| e["type"] == "response").count();

    let picked = project.ask("Falta a base.", "Qual a base?", "Base", &["dev", "main"], "dev");
    let typed = project.ask("Falta a base.", "Qual a base?", "Base", &["dev", "main"], "a main, por favor");

    assert!(picked.is_empty() && typed.is_empty(), "a common question says nothing: {picked} {typed}");
    let answers: Vec<Value> = project
        .events()
        .into_iter()
        .filter(|e| e["type"] == "message" && e.get("question").is_some())
        .map(|e| json!({"author": e["author"], "text": e["text"], "question": e["question"], "witness": e.get("witness")}))
        .collect();
    let answered = |text: &str| json!({"author": "user", "text": text, "question": "Qual a base?", "witness": null});
    assert_eq!(answers, [answered("dev"), answered("a main, por favor")]);
    let after = project.events().iter().filter(|e| e["type"] == "response").count();
    assert_eq!(after, before, "the question never becomes a response");
}
