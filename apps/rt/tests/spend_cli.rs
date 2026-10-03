// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! O gasto de cada dia pelo binário de verdade: o comando `run spend` e a barra
//! de status que lê o arquivo dele. A pasta do gasto e a de configuração do
//! Claude Code são de mentira, numa pasta temporária.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::domain::spend::previous_day;
use mustard_core::io::spend::today;
use serde_json::{json, Value};

/// O projeto `loja` num repositório git, com a pasta de configuração, a do
/// gasto e uma pasta pessoal falsa ao lado.
struct Scene {
    _dir: tempfile::TempDir,
    project: PathBuf,
    config: PathBuf,
    spend: PathBuf,
    home: PathBuf,
}

impl Scene {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let (project, home, config) = (dir.path().join("loja"), dir.path().join("home"), dir.path().join("config"));
        for path in [&project, &home, &config] {
            std::fs::create_dir_all(path).unwrap();
        }
        assert!(Command::new("git").args(["init", "-q"]).current_dir(&project).status().unwrap().success());
        std::fs::write(project.join("mustard.json"), "{}").unwrap();
        Self { spend: dir.path().join("spend"), _dir: dir, project, config, home }
    }

    /// Roda `mustard-rt` no projeto e devolve a saída padrão.
    fn rt(&self, args: &[&str], stdin: &str) -> String {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(args)
            .current_dir(&self.project)
            .env("CLAUDE_PROJECT_DIR", &self.project)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CLAUDE_CONFIG_DIR", &self.config)
            .env("MUSTARD_SPEND_DIR", &self.spend)
            .env("MUSTARD_CLAUDE_BIN", self.home.join("no-claude-here"))
            .env_remove("CLAUDE_PLUGIN_ROOT")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).into_owned()
    }

    /// O comando `run spend` com `args`, e a resposta dele já lida.
    fn spend(&self, args: &[&str]) -> Value {
        let out = self.rt(&[&["run", "spend"], args].concat(), "");
        serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("not JSON ({e}): {out}"))
    }

    /// Grava a conversa `s1` do projeto com as linhas `lines`.
    fn conversation(&self, lines: &[String]) {
        let dir = self.config.join("projects/loja");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s1.jsonl"), lines.join("\n") + "\n").unwrap();
    }

    fn ledger(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.spend.join("ledger.json")).unwrap()).unwrap()
    }
}

/// Uma resposta do modelo no dia `day`, gravada em `cwd`, com os usos de
/// ferramenta `tools`.
fn reply(cwd: &Path, id: &str, day: &str, tokens: u64, tools: &[(&str, Value)]) -> String {
    let content: Vec<Value> = tools
        .iter()
        .enumerate()
        .map(|(n, (name, input))| json!({"type": "tool_use", "id": format!("{id}-{n}"), "name": name, "input": input}))
        .collect();
    json!({"timestamp": format!("{day}T15:00:00Z"), "cwd": cwd.to_string_lossy(), "message": {
        "id": id, "model": "m", "usage": {"input_tokens": tokens, "output_tokens": 0}, "content": content}})
    .to_string()
}

/// As escritas da cópia preparada em `answer`, cada uma com o corpo do
/// documento lido do arquivo.
fn writes(answer: &Value) -> Vec<(Value, Value)> {
    let batches = answer["copy"]["spend"]["writes"].as_array().unwrap();
    let all = batches.iter().flat_map(|batch| batch.as_array().unwrap().clone());
    all.map(|write| {
        let body = serde_json::from_str(&std::fs::read_to_string(write["file_path"].as_str().unwrap()).unwrap()).unwrap();
        (write, body)
    })
    .collect()
}

/// A mesma resposta repetida em várias linhas conta os tokens uma vez; `Bash`
/// com `grep` e `Grep` são procuras de código, o `map search` e o `Read` não;
/// o custo do Jev gravado na spec entra na linha do dia, o da busca por
/// palavra e o da montagem da onda juntos; e uma conversa aberta dentro da
/// pasta `.claude` conta no projeto.
#[test]
fn the_command_counts_tokens_once_and_the_code_searches_of_each_day_and_project() {
    let scene = Scene::new();
    let yesterday = previous_day(&today()).unwrap();
    let tools = [
        ("Read", json!({"file_path": "src/a.rs"})),
        ("Bash", json!({"command": "grep -rn frete src"})),
        ("Bash", json!({"command": "mustard-rt run map search \"frete\""})),
        ("Grep", json!({"pattern": "x"})),
    ];
    let spec = scene.project.join(".claude/spec/uma-spec");
    scene.conversation(&[
        reply(&scene.project, "m1", &yesterday, 100, &tools),
        reply(&scene.project, "m1", &yesterday, 250, &tools),
        reply(&scene.project.join(".claude/spec"), "m2", &yesterday, 60, &[("Grep", json!({"pattern": "y"}))]),
    ]);
    let call = |id: u64, command: &str, tokens: u64, cost: u64| {
        json!({"v": 1, "id": id, "code": format!("X-CALL-{id:04}"), "at": format!("{yesterday}T10:00:00-03:00"),
            "type": "call", "author": "binary", "command": command, "tokens": tokens, "cost_micro_usd": cost})
        .to_string()
    };
    std::fs::create_dir_all(&spec).unwrap();
    let calls = [call(1, "word search", 1000, 900), call(2, "word search", 500, 400), call(3, "wave assembly", 200, 100)];
    std::fs::write(spec.join("spec.ndjson"), calls.join("\n") + "\n").unwrap();

    let answer = scene.spend(&[]);
    assert_eq!(answer["counted"]["rows"], json!(1), "{answer}");
    let (_, row) = writes(&answer).into_iter().next().expect("yesterday's row");
    assert_eq!((row["project"].clone(), row["day"].clone()), (json!("loja"), json!(yesterday)));
    assert_eq!(row["tokens"], json!(310), "the repeated response counts once, with its last line");
    assert_eq!(row["actions"], json!(5), "four tools of the first response and the one inside .claude");
    assert_eq!(row["code_searches"], json!(3), "grep in Bash and the two Grep, not the map search, not the Read");
    assert_eq!((row["jev_tokens"].clone(), row["jev_cost_micro_usd"].clone()), (json!(1700), json!(1400)));
}

/// Um dia fechado é contado uma vez e hoje, o dia aberto, vai como parcial e
/// nunca ao arquivo dos fechados: depois da cópia, o pedido seguinte leva só
/// hoje e o resumo, com a versão que o banco tem; e o `--republish` leva todos
/// os dias, sem versão.
#[test]
fn a_closed_day_is_counted_once_today_stays_out_of_the_file_and_republish_sends_every_day() {
    let scene = Scene::new();
    let (today, yesterday) = (today(), previous_day(&today()).unwrap());
    let before = previous_day(&yesterday).unwrap();
    let said = |extra: &[String]| {
        let mut all = vec![
            reply(&scene.project, "m1", &yesterday, 100, &[]),
            reply(&scene.project, "m2", &before, 70, &[]),
            reply(&scene.project, "m3", &today, 30, &[("Grep", json!({"pattern": "x"}))]),
        ];
        all.extend_from_slice(extra);
        scene.conversation(&all);
    };
    said(&[]);

    let first = scene.spend(&[]);
    assert_eq!(first["counted"]["rows"], json!(2), "only the closed days are counted: {first}");
    assert!(first["order"][0].as_str().unwrap().contains("page.html"), "the page is not published yet: {first}");
    let docs = writes(&first);
    let days: Vec<(Value, Value)> = docs.iter().filter(|(_, b)| b.get("day").is_some()).map(|(_, b)| (b["day"].clone(), b["partial"].clone())).collect();
    assert_eq!(days, [(json!(before), json!(false)), (json!(yesterday), json!(false)), (json!(today), json!(true))]);
    let summary = &docs.last().unwrap().1;
    assert_eq!((summary["today"]["tokens"].clone(), summary["yesterday"]["tokens"].clone()), (json!(30), json!(100)));
    let ledger = scene.ledger();
    assert_eq!(ledger["rows"].as_array().unwrap().len(), 2, "today stays out of the closed file: {ledger}");
    assert_eq!(ledger["counted_through"], json!(yesterday));

    scene.spend(&["--url", "https://claude.ai/code/artifact/gasto"]);
    said(&[reply(&scene.project, "m4", &yesterday, 5000, &[]), reply(&scene.project, "m5", &today, 70, &[])]);
    let second = scene.spend(&[]);
    assert_eq!(second["counted"], Value::Null, "a closed day is not counted twice: {second}");
    let again = writes(&second);
    let names: Vec<&str> = again.iter().map(|(w, _)| w["doc_id"].as_str().unwrap()).collect();
    assert!(names.len() == 2 && names[0].starts_with(&today) && names[1] == "current", "{names:?}");
    assert!(again.iter().all(|(w, _)| w["if_version"] == json!(1)), "both replace what the database has: {second}");
    assert_eq!(again[0].1["tokens"], json!(100), "today is counted again, with the new replies");
    let kept: Vec<u64> = scene.ledger()["rows"].as_array().unwrap().iter().map(|r| r["tokens"].as_u64().unwrap()).collect();
    assert_eq!(kept, [70, 100], "the closed days keep their first count");

    let all = scene.spend(&["--republish"]);
    assert_eq!(writes(&all).len(), 4, "the two closed days, today and the summary: {all}");
    assert!(writes(&all).iter().all(|(w, _)| w.get("if_version").is_none()), "the new page starts empty: {all}");
    assert!(all["order"][0].as_str().unwrap().contains("page.html"), "the order publishes the page again: {all}");
}

/// A barra de status mostra o consumo contra a média lido do arquivo dos dias
/// fechados: sete dias de 120 milhões de tokens e um último de 90 milhões dão
/// menos 25%, com o link do painel. Sem o arquivo, a barra não traz o
/// indicador, mesmo com conversas na máquina: ela só lê o arquivo.
#[test]
fn the_status_bar_shows_the_consumption_against_the_average_with_the_panel_link() {
    let scene = Scene::new();
    let panel = "https://claude.ai/code/artifact/painel-do-consumo";
    let payload = json!({"workspace": {"current_dir": scene.project.to_string_lossy()}, "model": {"display_name": "Opus 5"}})
        .to_string();
    let bar = |scene: &Scene| scene.rt(&["run", "statusline"], &payload);

    scene.conversation(&[reply(&scene.project, "m1", &previous_day(&today()).unwrap(), 90, &[])]);
    assert!(!bar(&scene).contains("consumo"), "no closed-days file, no indicator, however many conversations there are");
    assert!(!scene.spend.join("ledger.json").exists(), "the bar counts nothing and writes nothing");

    let row = |day: &str, tokens: u64| json!({"day": day, "project": "loja", "tokens": tokens, "actions": 300});
    let mut rows: Vec<Value> = (20..=26).map(|day| row(&format!("2026-09-{day}"), 120_000_000)).collect();
    rows.push(row("2026-09-27", 90_000_000));
    std::fs::create_dir_all(&scene.spend).unwrap();
    std::fs::write(scene.spend.join("ledger.json"), json!({"counted_through": "2026-09-27", "url": panel, "rows": rows}).to_string())
        .unwrap();

    let shown = bar(&scene);
    assert!(
        shown.contains(&format!("\u{1b}]8;;{panel}\u{1b}\\")) && shown.contains("consumo \u{2212}25% vs média de 7 dias"),
        "the label links the panel: {shown:?}"
    );
    std::fs::remove_file(scene.spend.join("ledger.json")).unwrap();
    assert!(!bar(&scene).contains("consumo"), "the file is gone, so is the indicator");
}
