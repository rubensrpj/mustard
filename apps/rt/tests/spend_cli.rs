// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! O gasto de cada dia, pelo binário de verdade: o comando `run spend` e o
//! início da sessão que o pede.
//!
//! O dia de hoje é o relógio da máquina, então as conversas falsas se datam
//! por "ontem" e "anteontem" no fuso do gasto. A pasta do gasto e a pasta de
//! configuração do Claude Code são de mentira, dentro de uma pasta temporária:
//! nenhum teste lê a máquina de quem roda a suíte.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::domain::spend::previous_day;
use mustard_core::io::spend::today;
use serde_json::{json, Value};

/// A cena de um teste: o projeto, a pasta de configuração do Claude Code, a
/// pasta do gasto e uma pasta pessoal falsa.
struct Scene {
    _dir: tempfile::TempDir,
    project: PathBuf,
    config: PathBuf,
    spend: PathBuf,
    home: PathBuf,
}

impl Scene {
    /// Um projeto com `mustard.json` num repositório git, sem conversa
    /// nenhuma.
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("loja");
        let home = dir.path().join("home");
        let (config, spend) = (dir.path().join("config"), dir.path().join("spend"));
        for path in [&project, &home, &config] {
            std::fs::create_dir_all(path).unwrap();
        }
        let ok = Command::new("git").args(["init", "-q"]).current_dir(&project).status().unwrap().success();
        assert!(ok, "git init");
        std::fs::write(project.join("mustard.json"), "{}").unwrap();
        Self { _dir: dir, project, config, spend, home }
    }

    /// Roda `mustard-rt` no projeto, com o gasto e a configuração da cena, e
    /// devolve a saída padrão e o código de saída.
    fn rt(&self, args: &[&str], stdin: &str) -> (String, Option<i32>) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(args)
            .current_dir(&self.project)
            .env("CLAUDE_PROJECT_DIR", &self.project)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CLAUDE_CONFIG_DIR", &self.config)
            .env("MUSTARD_SPEND_DIR", &self.spend)
            .env_remove("CLAUDE_PLUGIN_ROOT")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .env("MUSTARD_CLAUDE_BIN", self.home.join("no-claude-here"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(stdin.as_bytes());
        }
        let out = child.wait_with_output().expect("the binary finishes");
        (String::from_utf8_lossy(&out.stdout).into_owned(), out.status.code())
    }

    /// O comando `run spend` com `args`, e a resposta dele já lida.
    fn spend(&self, args: &[&str]) -> Value {
        let mut all = vec!["run", "spend"];
        all.extend_from_slice(args);
        let (out, code) = self.rt(&all, "");
        let answer: Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("not JSON ({e}): {out}"));
        assert_eq!(code, Some(0), "{answer}");
        answer
    }

    /// O início da sessão e o texto que ele põe na janela.
    fn session_start(&self) -> String {
        let payload = json!({
            "hook_event_name": "SessionStart", "source": "startup", "session_id": "s-gasto",
            "cwd": self.project.to_string_lossy(),
        })
        .to_string();
        let (out, code) = self.rt(&["on", "SessionStart"], &payload);
        assert_eq!(code, Some(0), "{out}");
        if out.trim().is_empty() {
            return String::new();
        }
        let answer: Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("not JSON ({e}): {out}"));
        answer["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_default().to_string()
    }

    /// A conversa `session` do projeto, com as linhas `lines`.
    fn conversation(&self, session: &str, lines: &[String]) -> PathBuf {
        let dir = self.config.join("projects").join("loja");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{session}.jsonl"));
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    /// Uma resposta do modelo no dia `day`, gravada no projeto da cena, com os
    /// tokens e os usos de ferramenta `tools`.
    fn reply(&self, id: &str, day: &str, seconds: u32, tokens: u64, tools: &[(&str, Value)]) -> String {
        reply(&self.project, id, day, seconds, tokens, tools)
    }

    /// O arquivo do gasto da máquina, lido.
    fn ledger(&self) -> Value {
        let text = std::fs::read_to_string(self.spend.join("ledger.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

/// Uma resposta do modelo no dia `day` às 15:00 UTC mais `seconds`, gravada em
/// `cwd`.
fn reply(cwd: &Path, id: &str, day: &str, seconds: u32, tokens: u64, tools: &[(&str, Value)]) -> String {
    let content: Vec<Value> = tools
        .iter()
        .enumerate()
        .map(|(n, (name, input))| json!({"type": "tool_use", "id": format!("{id}-{n}"), "name": name, "input": input}))
        .collect();
    json!({"timestamp": format!("{day}T15:00:{seconds:02}Z"), "cwd": cwd.to_string_lossy(), "message": {
        "id": id, "model": "m", "usage": {"input_tokens": tokens, "output_tokens": 0}, "content": content}})
    .to_string()
}

/// As escritas dos lotes da resposta `answer`, na ordem em que a resposta as
/// manda.
fn writes(answer: &Value) -> Vec<Value> {
    answer["copy"]["spend"]["writes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|batch| batch.as_array().unwrap().iter().cloned())
        .collect()
}

/// O corpo, lido do arquivo, do documento de cada escrita dos lotes da resposta
/// `answer`, na ordem em que a resposta os manda.
fn documents(answer: &Value) -> Vec<Value> {
    answer["copy"]["spend"]["writes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|batch| batch.as_array().unwrap().iter())
        .map(|write| {
            let file = write["file_path"].as_str().unwrap();
            assert!(Path::new(file).is_absolute(), "the batch carries the absolute path: {file}");
            serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
        })
        .collect()
}

/// A mesma resposta repetida em várias linhas conta os tokens uma vez; `Bash`
/// com `grep` e `Grep` são procuras de código e o `mustard-rt run map search`
/// não é; `Read` vai para as leituras; o `word search` gravado na spec entra
/// como busca do Mustard, e o que voltou com zero como vazio.
#[test]
fn the_command_counts_tokens_once_and_sorts_every_tool_into_its_column() {
    let scene = Scene::new();
    let (yesterday, before) = (previous_day(&today()).unwrap(), previous_day(&previous_day(&today()).unwrap()).unwrap());
    let tools = [
        ("Read", json!({"file_path": "src/a.rs"})),
        ("Bash", json!({"command": "grep -rn frete src"})),
        ("Bash", json!({"command": "mustard-rt run map search \"frete\""})),
        ("Grep", json!({"pattern": "x"})),
    ];
    scene.conversation(
        "s1",
        &[
            scene.reply("m1", &yesterday, 0, 100, &tools),
            scene.reply("m1", &yesterday, 1, 250, &tools),
            scene.reply("m2", &before, 0, 70, &[]),
        ],
    );
    let spec = scene.project.join(".claude/spec/uma-spec");
    std::fs::create_dir_all(&spec).unwrap();
    let call = |id: u64, returned: u64, tokens: u64, cost: u64| {
        json!({"v": 1, "id": id, "code": format!("X-CALL-{id:04}"), "at": format!("{yesterday}T10:00:00-03:00"),
            "type": "call", "author": "binary", "command": "word search", "returned": returned,
            "tokens": tokens, "cost_micro_usd": cost})
        .to_string()
    };
    std::fs::write(spec.join("spec.ndjson"), [call(1, 2, 1000, 900), call(2, 0, 500, 400)].join("\n") + "\n").unwrap();

    let answer = scene.spend(&[]);
    assert_eq!(answer["counted"]["rows"], json!(2), "{answer}");
    let rows = documents(&answer);
    let last = rows.iter().find(|row| row["day"] == json!(yesterday)).expect("yesterday's row");
    assert_eq!(last["project"], json!("loja"));
    assert_eq!(last["tokens"], json!(250), "the repeated response counts once, with its last line");
    assert_eq!(last["actions"], json!(4));
    assert_eq!(last["code_searches"], json!(2), "grep in Bash and Grep, not the mustard-rt call, not the Read");
    assert_eq!(last["file_reads"], json!(1), "the Read goes to the reads column");
    assert_eq!(last["mustard_searches"], json!(3), "the map search in Bash and the two word searches");
    assert_eq!(last["empty_searches"], json!(1), "the word search that returned zero");
    assert_eq!((last["jev_tokens"].clone(), last["jev_cost_micro_usd"].clone()), (json!(1500), json!(1300)));
}

/// Uma busca por comando, com o evento que ela grava na spec, conta uma vez
/// na linha do dia, com os tokens e o custo do evento.
#[test]
fn a_search_by_command_with_its_event_counts_once() {
    let scene = Scene::new();
    let yesterday = previous_day(&today()).unwrap();
    let search = [("Bash", json!({"command": "mustard-rt run map search \"frete\""}))];
    scene.conversation("s1", &[scene.reply("m1", &yesterday, 0, 100, &search)]);
    let spec = scene.project.join(".claude/spec/uma-spec");
    std::fs::create_dir_all(&spec).unwrap();
    // A resposta é carimbada às 15:00:00 UTC, e a busca grava a chamada dois segundos depois.
    let event = json!({"v": 1, "id": 1, "code": "X-CALL-0001", "at": format!("{yesterday}T12:00:02-03:00"),
        "type": "call", "author": "binary", "command": "word search", "ms": 1500, "returned": 2,
        "tokens": 1000, "cost_micro_usd": 900});
    std::fs::write(spec.join("spec.ndjson"), event.to_string() + "\n").unwrap();

    let answer = scene.spend(&[]);
    let rows: Vec<Value> = documents(&answer).into_iter().filter(|doc| doc.get("day").is_some()).collect();
    assert_eq!(rows.len(), 1, "{answer}");
    assert_eq!(rows[0]["mustard_searches"], json!(1), "the command and its event are one search: {answer}");
    assert_eq!((rows[0]["jev_tokens"].clone(), rows[0]["jev_cost_micro_usd"].clone()), (json!(1000), json!(900)));
}

/// Uma conversa aberta dentro da pasta `.claude` do projeto, ou numa pasta
/// dela, conta no projeto: a conta não para na `.claude` nem a toma por
/// projeto.
#[test]
fn a_conversation_opened_inside_the_dot_claude_folder_counts_for_its_project() {
    let scene = Scene::new();
    let yesterday = previous_day(&today()).unwrap();
    let inside = scene.project.join(".claude");
    scene.conversation(
        "s-dentro",
        &[
            reply(&inside, "d1", &yesterday, 0, 40, &[]),
            reply(&inside.join("spec"), "d2", &yesterday, 1, 60, &[("Grep", json!({"pattern": "x"}))]),
        ],
    );
    let answer = scene.spend(&[]);
    let rows: Vec<Value> = documents(&answer).into_iter().filter(|doc| doc.get("day").is_some()).collect();
    assert_eq!(rows.len(), 1, "{answer}");
    assert_eq!(rows[0]["project"], json!("loja"));
    assert_eq!((rows[0]["tokens"].clone(), rows[0]["code_searches"].clone()), (json!(100), json!(1)));
}

/// Um dia fechado é contado uma vez e guardado no arquivo da máquina: depois
/// da primeira conta, uma resposta nova numa conversa de dia fechado não muda
/// a linha; e o `--republish` manda todos os dias, mesmo os já copiados.
#[test]
fn a_closed_day_is_not_recounted_and_republish_sends_every_day() {
    let scene = Scene::new();
    let (yesterday, before) = (previous_day(&today()).unwrap(), previous_day(&previous_day(&today()).unwrap()).unwrap());
    let path = scene.conversation(
        "s1",
        &[scene.reply("m1", &yesterday, 0, 100, &[]), scene.reply("m2", &before, 0, 70, &[])],
    );

    let first = scene.spend(&[]);
    assert_eq!(first["counted"]["rows"], json!(2), "{first}");
    assert_eq!(first["copy"]["spend"]["rows"], json!(2));
    let order = first["order"].as_array().unwrap();
    assert!(order[0].as_str().unwrap().contains("page.html"), "the page is not published yet: {first}");
    scene.spend(&["--url", "https://claude.ai/code/artifact/gasto"]);
    scene.spend(&["--copied"]);

    // A resposta que chega depois a um dia já contado não o conta de novo.
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str(&scene.reply("m3", &yesterday, 5, 5000, &[]));
    text.push('\n');
    std::fs::write(&path, text).unwrap();
    let second = scene.spend(&[]);
    assert_eq!(second["counted"], Value::Null, "nothing to count: {second}");
    assert_eq!(second["copy"]["spend"]["rows"], json!(0), "every closed line was copied");
    let only: Vec<Value> = documents(&second).iter().map(|doc| doc["day"].clone()).collect();
    assert_eq!(only, [Value::Null], "only the summary goes again");
    let kept: Vec<u64> = scene.ledger()["rows"].as_array().unwrap().iter().map(|r| r["tokens"].as_u64().unwrap()).collect();
    assert_eq!(kept, [70, 100], "the closed days keep their first count");

    // O `--republish` leva todos os dias, e manda publicar outra página.
    let again = scene.spend(&["--republish"]);
    assert_eq!(again["copy"]["spend"]["rows"], json!(2), "{again}");
    let days: Vec<Value> = documents(&again).iter().filter_map(|row| row.get("day").cloned()).collect();
    assert_eq!(days, [json!(before), json!(yesterday)], "every day, oldest first");
    assert!(writes(&again).iter().all(|write| write.get("if_version").is_none()), "the new page starts empty: {again}");
    assert!(again["order"][0].as_str().unwrap().contains("page.html"), "the order publishes the page again: {again}");
}

/// O comando roda sem spec aberta (sem a pasta de specs do projeto e sem spec
/// ativa), e hoje, o dia aberto, vai à cópia marcado como parcial mas nunca ao
/// arquivo dos dias fechados: o arquivo para em ontem, hoje é recontado a cada
/// pedido, e depois de uma cópia gravada a linha de hoje e o resumo voltam com
/// a versão que o banco tem, sem as linhas fechadas já copiadas.
#[test]
fn the_command_runs_without_an_open_spec_and_today_never_reaches_the_closed_file() {
    let scene = Scene::new();
    assert!(!scene.project.join(".claude/spec").exists(), "no spec in this project");
    let (today, yesterday) = (today(), previous_day(&today()).unwrap());
    let path = scene.conversation(
        "s1",
        &[
            scene.reply("m1", &yesterday, 0, 100, &[]),
            scene.reply("m2", &today, 0, 30, &[("Grep", json!({"pattern": "x"}))]),
        ],
    );

    let first = scene.spend(&[]);
    assert_eq!(first["ok"], json!(true), "{first}");
    assert_eq!(first["counted"]["rows"], json!(1), "only the closed day is counted: {first}");
    let docs = documents(&first);
    let partial: Vec<(Value, Value)> = docs.iter().filter(|doc| doc.get("day").is_some()).map(|doc| (doc["day"].clone(), doc["partial"].clone())).collect();
    assert_eq!(partial, [(json!(yesterday), json!(false)), (json!(today), json!(true))], "the open day goes as partial");
    let summary = docs.last().unwrap();
    assert_eq!((summary["today"]["day"].clone(), summary["today"]["tokens"].clone()), (json!(today), json!(30)));
    assert_eq!(summary["yesterday"]["tokens"], json!(100));
    let ledger = scene.ledger();
    let rows = ledger["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{ledger}");
    assert_eq!(rows[0]["day"], json!(yesterday));
    assert_eq!(ledger["counted_through"], json!(yesterday), "the closed file stops at yesterday");

    scene.spend(&["--url", "https://claude.ai/code/artifact/gasto"]);
    scene.spend(&["--copied"]);
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str(&scene.reply("m3", &today, 9, 70, &[]));
    text.push('\n');
    std::fs::write(&path, text).unwrap();

    let second = scene.spend(&[]);
    let again = writes(&second);
    let names: Vec<&str> = again.iter().map(|w| w["doc_id"].as_str().unwrap()).collect();
    assert_eq!(names.len(), 2, "today and the summary, not the closed line: {second}");
    assert!(names[0].starts_with(&today) && names[1] == "current", "{names:?}");
    assert!(again.iter().all(|w| w["if_version"] == json!(1)), "both replace what the database has: {second}");
    let docs = documents(&second);
    assert_eq!(docs[0]["tokens"], json!(100), "today is counted again, with the new reply");
    assert_eq!(docs[0]["partial"], json!(true));
    assert_eq!(scene.ledger()["rows"].as_array().unwrap().len(), 1, "today still never reaches the closed file");
}

/// O início da sessão manda rodar o gasto em toda sessão, sem depender de
/// spec aberta nem do que já foi contado ou copiado: a ordem entra na janela
/// da primeira sessão, da segunda do mesmo dia e de uma conta toda em dia.
#[test]
fn the_session_start_asks_for_the_spend_every_time_without_an_open_spec() {
    let scene = Scene::new();
    let order = "mustard-rt run spend";
    assert!(!scene.project.join(".claude/spec").exists());
    assert!(scene.session_start().contains(order), "a machine that never counted gets the order");
    assert!(scene.session_start().contains(order), "the second session of the day gets it again");

    let yesterday = previous_day(&today()).unwrap();
    std::fs::create_dir_all(&scene.spend).unwrap();
    std::fs::write(scene.spend.join("ledger.json"), json!({"counted_through": yesterday}).to_string()).unwrap();
    assert!(scene.session_start().contains(order), "an up-to-date count still asks: today is recounted");
}

/// Um projeto sem `mustard.json` não recebe a ordem, e nada é escrito.
#[test]
fn a_project_without_a_config_never_gets_the_spend_order() {
    let scene = Scene::new();
    std::fs::remove_file(scene.project.join("mustard.json")).unwrap();
    assert!(!scene.session_start().contains("mustard-rt run spend"));
    assert!(!scene.spend.join("ledger.json").exists(), "nothing was written");
}
