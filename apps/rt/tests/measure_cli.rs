// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A medição do uso real pelo binário de verdade: o comando `run measure`,
//! com as conversas do Claude Code, a pasta do gasto e a pasta pessoal numa
//! pasta temporária.

#[path = "support/binary.rs"]
mod binary;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

/// Um projeto com `mustard.json` num repositório git, em `folder`.
fn project(folder: &Path) -> PathBuf {
    std::fs::create_dir_all(folder).unwrap();
    let git = |args: &[&str]| assert!(Command::new("git").args(args).current_dir(folder).status().unwrap().success());
    git(&["init", "-q"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "start"]);
    std::fs::write(folder.join("mustard.json"), "{}").unwrap();
    folder.to_path_buf()
}

/// A conversa `file` da pasta de configuração `config`: em cada dia, uma
/// resposta aberta em `cwd` com as ações e os tokens dele.
fn conversation(config: &Path, file: &str, cwd: &Path, days: &[(&str, u64, u64)]) {
    let lines: Vec<String> = days
        .iter()
        .map(|&(day, actions, tokens)| {
            let tool = |n: u64| json!({"type": "tool_use", "id": format!("{file}-{day}-{n}"), "name": "Read", "input": {}});
            let usage = json!({"input_tokens": tokens, "output_tokens": 0});
            let message = json!({"id": format!("{file}-{day}"), "model": "m", "usage": usage, "content": (0..actions).map(tool).collect::<Vec<_>>()});
            json!({"timestamp": format!("{day}T15:00:00Z"), "cwd": cwd.to_string_lossy(), "message": message}).to_string()
        })
        .collect();
    let dir = config.join("projects").join(file);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("s1.jsonl"), lines.join("\n") + "\n").unwrap();
}

/// A resposta de `run measure` com `args`, rodado em `cwd`, com as conversas
/// de `config` e a pasta do gasto e a pessoal dentro de `dir`.
fn measure(dir: &Path, config: &Path, cwd: &Path, args: &[&str]) -> Value {
    let home = dir.join("home");
    let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args([&["run", "measure"], args].concat())
        .current_dir(cwd)
        .env("CLAUDE_PROJECT_DIR", cwd)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("CLAUDE_CONFIG_DIR", config)
        .env("MUSTARD_SPEND_DIR", dir.join("spend"))
        .env("MUSTARD_COPIES_DIR", dir.join("copias"))
        .env("MUSTARD_CLAUDE_BIN", home.join("no-claude-here"))
        .env_remove("CLAUDE_PLUGIN_ROOT")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .output()
        .expect("the binary runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let values = serde_json::Deserializer::from_str(&text).into_iter::<Value>().collect::<Result<Vec<_>, _>>();
    let mut values = values.unwrap_or_else(|e| panic!("not JSON ({e}): {text}"));
    values.pop().unwrap_or_else(|| panic!("no answer: {text}"))
}

/// Uma chamada de uma conversa: o código, o instante, a ferramenta, a entrada,
/// o contexto que o gancho pôs antes dela e o resultado, com a marca de erro.
type Call<'a> = (&'a str, &'a str, &'a str, Value, Option<String>, String, bool);

/// Grava em `config` a conversa `file`, aberta em `cwd`, com as chamadas
/// `calls`, cada uma com a nota do gancho antes dela, quando há, e o resultado.
fn calls_conversation(config: &Path, file: &str, cwd: Option<&Path>, calls: &[Call]) {
    let lines = calls.iter().flat_map(|(id, at, name, input, note, output, error)| {
        let tool = json!({"type": "tool_use", "id": id, "name": name, "input": input});
        let used = json!({"type": "assistant", "timestamp": at, "cwd": cwd, "message": {"content": [tool]}});
        let note = note.as_ref().map(|said| {
            let hook = format!("PreToolUse:{name}");
            let note = json!({"type": "hook_additional_context", "hookName": hook, "toolUseID": id, "content": [said]});
            json!({"type": "attachment", "attachment": note})
        });
        let result = json!({"type": "tool_result", "tool_use_id": id, "content": output, "is_error": error});
        let answered = json!({"type": "user", "message": {"content": [result]}});
        [Some(used), note, Some(answered)].into_iter().flatten().map(|line| line.to_string() + "\n")
    });
    let path = config.join("projects").join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.collect::<String>()).unwrap();
}

/// Os dias do modelo aprovado — 29 e 30/09 com 14.222 ações, 02 e 03/10 com
/// 8.663 — e cinco dias de mil ações antes deles, a cem mil tokens cada.
const DAYS: [(&str, u64, u64); 10] = [
    ("2026-09-23", 1_000, 100_000_000),
    ("2026-09-24", 1_000, 100_000_000),
    ("2026-09-25", 1_000, 100_000_000),
    ("2026-09-26", 1_000, 100_000_000),
    ("2026-09-27", 1_000, 100_000_000),
    ("2026-09-29", 7_000, 900_000_000),
    ("2026-09-30", 7_222, 877_000_000),
    ("2026-10-01", 9_000, 999_000_000),
    ("2026-10-02", 4_000, 600_000_000),
    ("2026-10-03", 4_663, 630_000_000),
];

/// A frase de quando nenhuma conversa tem script em volta do Mustard nem
/// edição por script desde a marca.
const NO_SCRIPTS: &str = "Nenhum script em volta do Mustard e nenhuma edição por script desde a marca.";

/// A resposta traz a tabela do gasto e a frase do veredito, no idioma do
/// projeto: dois dias de cada lado, como no modelo aprovado, ainda não dão
/// para dizer; cinco de cada lado dão; a marca de agora, sem dia depois, diz
/// que o dia dela fica fora e que o dia seguinte é o primeiro que pode contar,
/// com o antes pronto. Sem marca, a medição começa na próxima sessão; o
/// instante que não se lê é recusado. Nada vai ao arquivo do gasto nem à spec.
#[test]
fn the_answer_carries_the_spend_table_and_the_verdict_in_the_project_language() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    let root = project(&dir.path().join("loja"));
    conversation(&config, "loja", &root, &DAYS);
    let language = |text: &str| {
        std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{text}"}}}}"#)).unwrap();
    };
    let text = |lines: &[&str]| lines.join("\n");

    let too_early = [
        "| | antes (29 e 30/09) | depois (02 e 03/10) |",
        "|---|---:|---:|",
        "| dias contados | 2 | 2 |",
        "| ações do Claude | 14.222 | 8.663 |",
        "| tokens | 1.777 milhões | 1.230 milhões |",
        "| **tokens por ação** | **125 mil** | **142 mil** |",
        "",
        "Ainda não dá para dizer. São 2 dias contados antes e 2 depois, e o mínimo é 5 de cada lado: faltam 3 antes \
         e 3 depois. Até aqui, cada ação custou 14% mais depois da instalação.",
        "",
        NO_SCRIPTS,
    ];
    let too_early_en = [
        "| | before (29 and 30/09) | after (02 and 03/10) |",
        "|---|---:|---:|",
        "| counted days | 2 | 2 |",
        "| Claude actions | 14,222 | 8,663 |",
        "| tokens | 1,777 million | 1,230 million |",
        "| **tokens per action** | **125 thousand** | **142 thousand** |",
        "",
        "Too early to tell. There are 2 counted days before and 2 after, and the minimum is 5 on each side: 3 are \
         missing before and 3 after. So far, each action cost 14% more after the installation.",
        "",
        "No script around Mustard and no edit by script since the mark.",
    ];
    let ready = [
        "| | antes (23, 24, 25, 26 e 27/09) | depois (29, 30/09, 01, 02 e 03/10) |",
        "|---|---:|---:|",
        "| dias contados | 5 | 5 |",
        "| ações do Claude | 5.000 | 31.885 |",
        "| tokens | 500 milhões | 4.006 milhões |",
        "| **tokens por ação** | **100 mil** | **126 mil** |",
        "",
        "A comparação vale, com 5 dias contados antes e 5 depois: cada ação custou 26% mais depois da instalação.",
        "",
        NO_SCRIPTS,
    ];
    let no_mark_pt = "A medição começa na próxima sessão, quando esta versão do Mustard deixa a marca dela neste projeto.";
    let no_mark_en = "The measurement starts at the next session, when this Mustard version leaves its mark on this project.";
    let model_mark = ["--since", "2026-10-01T21:03:00-03:00"];
    let short = json!({"kind": "too-early", "missing_before": 3, "missing_after": 3});
    // O idioma, o `--since`, o veredito e o texto esperados.
    let cases: [(&str, &[&str], Value, String); 5] = [
        ("pt-BR", &[], Value::Null, no_mark_pt.to_string()),
        ("en-US", &[], Value::Null, no_mark_en.to_string()),
        ("pt-BR", &model_mark, short.clone(), text(&too_early)),
        ("en-US", &model_mark, short, text(&too_early_en)),
        ("pt-BR", &["--since", "2026-09-28"], json!({"kind": "ready"}), text(&ready)),
    ];
    for (lang, args, verdict, expected) in cases {
        language(lang);
        let answer = measure(dir.path(), &config, &root, args);
        let said = (answer["project"].clone(), answer["verdict"].clone());
        assert_eq!(said, (json!("loja"), verdict), "{lang} {args:?}: {answer}");
        assert_eq!(answer["text"], json!(expected), "{lang} {args:?}");
    }

    language("pt-BR");
    let refused = measure(dir.path(), &config, &root, &["--since", "01/10"]);
    assert_eq!((refused["ok"].clone(), refused["reason"].clone()), (json!(false), json!("not-an-instant")), "{refused}");
    mustard_core::io::measure::record(&root, "1.0 (abc)").unwrap();
    let unused = measure(dir.path(), &config, &root, &[]);
    let said = (unused["mark"]["version"].clone(), unused["verdict"].clone());
    assert_eq!(said, (json!("1.0 (abc)"), json!({"kind": "not-used-yet"})), "{unused}");
    // O dia da marca de agora no fuso de -03:00 do gasto e o dia seguinte.
    let at = chrono::DateTime::parse_from_rfc3339(unused["mark"]["at"].as_str().unwrap()).unwrap();
    let mark_day = at.with_timezone(&chrono::FixedOffset::west_opt(3 * 3600).unwrap()).date_naive();
    let shown = |day: chrono::NaiveDate| day.format("%d/%m").to_string();
    let first_day = shown(mark_day.succ_opt().unwrap());
    let verdict = format!(
        "Nenhum dia depois da marca foi contado ainda. O dia da marca, {}, fica fora da conta. O primeiro que \
         pode contar é {first_day}. Um dia entra quando fecha com 100 ações ou mais. A coluna de antes já está \
         pronta.",
        shown(mark_day)
    );
    let not_used_yet = [
        "| | antes (29, 30/09, 01, 02 e 03/10) | depois |",
        "|---|---:|---:|",
        "| dias contados | 5 | 0 |",
        "| ações do Claude | 31.885 | sem uso |",
        "| tokens | 4.006 milhões | sem uso |",
        "| **tokens por ação** | **126 mil** | sem uso |",
        "",
        &verdict,
        "",
        NO_SCRIPTS,
    ];
    assert_eq!(unused["text"], json!(text(&not_used_yet)));
    assert!(!dir.path().join("spend").exists(), "the measurement writes nothing to the spend folder");
    assert!(!root.join(".claude").join("spec").exists(), "the measurement writes nothing to a spec");
}

/// A medição conta só a pasta do projeto em que roda, com a cópia de onda
/// dele: outro projeto com o mesmo nome de pasta não soma, e a medição feita
/// de dentro da cópia é a do projeto dela.
#[test]
fn a_project_with_the_same_folder_name_does_not_add_to_the_measurement() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    let (shop, other) = (project(&dir.path().join("a").join("loja")), project(&dir.path().join("b").join("loja")));
    let copy = dir.path().join("copies").join("w1");
    let added = Command::new("git").args(["worktree", "add", "-q", "--detach"]).arg(&copy).current_dir(&shop).status();
    assert!(added.unwrap().success() && !copy.join("mustard.json").exists(), "the wave copy carries no mustard.json");
    conversation(&config, "shop", &shop, &[("2026-09-29", 100, 10_000_000), ("2026-10-02", 100, 10_000_000)]);
    conversation(&config, "copy", &copy, &[("2026-10-02", 100, 30_000_000)]);
    conversation(&config, "other", &other, &[("2026-09-29", 100, 50_000_000), ("2026-10-02", 100, 50_000_000)]);

    let sides = |answer: &Value| {
        let side = |key: &str| {
            let side = &answer[key];
            (side["days"].clone(), side["actions"].clone(), side["tokens"].clone())
        };
        (answer["project"].clone(), side("before"), side("after"))
    };
    let since = ["--since", "2026-10-01"];
    let shop_sides = (
        json!("loja"),
        (json!(["2026-09-29"]), json!(100), json!(10_000_000)),
        (json!(["2026-10-02"]), json!(200), json!(40_000_000)),
    );
    assert_eq!(sides(&measure(dir.path(), &config, &shop, &since)), shop_sides, "the shop and its wave copy");
    assert_eq!(sides(&measure(dir.path(), &config, &copy, &since)), shop_sides, "measured from inside the wave copy");
    let other_sides = (
        json!("loja"),
        (json!(["2026-09-29"]), json!(100), json!(50_000_000)),
        (json!(["2026-10-02"]), json!(100), json!(50_000_000)),
    );
    assert_eq!(sides(&measure(dir.path(), &config, &other, &since)), other_sides, "the other project of the same name");
}

/// Desde a marca, a medição conta, por conversa e por papel (condutor,
/// agente de onda, outro agente), as chamadas que passam a resposta do
/// Mustard por `python`, `jq` ou `grep`, no mesmo comando ou pelo arquivo em
/// que ela foi gravada, e as que montam por script os dados de uma gravação,
/// no mesmo comando, pela variável ou pelo arquivo do `--json @arquivo`; o
/// script num heredoc vale pelo corpo dele, também depois de outro heredoc
/// com o mesmo delimitador. O arquivo se casa pelo caminho nos dois lados: o
/// relativo, a partir da pasta a que o `cd` da chamada levou; o que começa por
/// variável, pelo texto; e o `--json=@arquivo`, com o igual. Conta também as
/// edições por script, pelas formas que o agente de onda tem recusadas, com o
/// programa passado pelo cano, o interpretador rodado por um lançador e a
/// cópia de onda que já saiu do disco lida pelo checkout. A
/// linha repetida no arquivo conta uma vez; ficam fora a chamada antes da
/// marca, a resposta lida sem script, a busca num arquivo do código, o
/// programa sem caminho do projeto, o módulo instalado, a cópia gravada fora
/// do projeto e a conversa de outro projeto.
#[test]
fn the_measure_counts_scripts_around_mustard_and_script_edits() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    let (root, other) = (project(&dir.path().join("loja")), project(&dir.path().join("outra")));
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/frete.rs"), "fn calcular() {}\n").unwrap();
    let scratch = dir.path().join("scratch").display().to_string();
    // A pasta das cópias existe, mas a cópia da onda já saiu do disco.
    let copies = dir.path().join("copias").join(mustard_core::io::wave_prompt::copies_dir(&root).file_name().unwrap());
    std::fs::create_dir_all(&copies).unwrap();
    let gone = copies.join("obra").join("z").display().to_string();
    let at = "2026-10-02T10:00:00Z";
    let bash = |id: &'static str, command: String| -> Call<'static> {
        (id, at, "Bash", json!({ "command": command }), None, String::new(), false)
    };
    let piped = bash("piped", "mustard-rt run read request-1 | python3 -c 'import sys; print(sys.stdin.read())'".into());
    let grep_tool = json!({"pattern": "onda", "path": format!("{scratch}/answer.json")});
    let looped = format!(
        "while read -r line; do t=$(printf '%s' \"$line\" | python3 -c 'import json,sys; print(json.dumps({{\"text\": \
         sys.stdin.read()}}))'); mustard-rt run write task --json \"$t\"; done < {scratch}/lines.txt"
    );
    let built = format!(
        "cat > {scratch}/nota.txt <<'EOF'\nnota\nEOF\npython3 - <<'EOF'\nimport json\njson.dump({{\"wave\": 1}}, open(\"{scratch}/step.json\", \"w\"))\nEOF"
    );
    let conductor: Vec<Call> = vec![
        ("early", "2026-09-30T10:00:00Z", "Bash", json!({"command": "mustard-rt run read request-1 | jq ."}), None, String::new(), false),
        piped.clone(),
        piped,
        bash("substituted", "jq -r .text <(mustard-rt run panel)".into()),
        bash("saved", format!("mustard-rt run read dispatch-1 > {scratch}/answer.json")),
        bash("grepped", format!("grep -n onda {scratch}/answer.json")),
        bash("catted", format!("cat {scratch}/answer.json | jq .waves")),
        ("grep-tool", at, "Grep", grep_tool, None, String::new(), false),
        bash("built", built),
        bash("consumed", format!("mustard-rt run write step --json @{scratch}/step.json")),
        bash("looped", looped),
        bash("nested", "mustard-rt run write step --json \"$(jq -n '{wave: 1}')\"".into()),
        // O arquivo gravado com caminho relativo depois de `cd` e lido por outro caminho relativo, de outra pasta.
        bash("relative-saved", format!("cd {scratch} && mustard-rt run read dispatch-2 > rel.json")),
        bash("relative-read", format!("cd {scratch}/sub && grep -n onda ../rel.json")),
        // O arquivo com caminho que começa por variável, gravado e lido de pastas diferentes.
        bash("variable-built", format!("S={scratch}; jq -n '{{wave: 2}}' > $S/x.json")),
        bash("variable-consumed", format!("S={scratch}; cd {scratch}/other && mustard-rt run write step --json @$S/x.json")),
        // O arquivo lido pelo `--json=@arquivo`, com o igual.
        bash("equals-built", format!("jq -n '{{wave: 3}}' > {scratch}/eq.json")),
        bash("equals-consumed", format!("mustard-rt run write step --json=@{scratch}/eq.json")),
        bash("plain-read", format!("cat {scratch}/answer.json")),
        bash("source-grep", "grep -n calcular src/frete.rs".into()),
        bash("folded", "mustard-rt run read request-1 2>&1 | fold -w 200".into()),
        bash("conductor-edit", "sed -i 's/a/b/' src/frete.rs".into()),
    ];
    let wave: Vec<Call> = vec![
        bash("gone-copy", format!("cd {gone} && python3 - <<'EOF'\np = \"src/frete.rs\"\nopen(p, \"w\").write(\"x\")\nEOF")),
        bash("in-place", "perl -i -pe 's/a/b/' src/frete.rs".into()),
        bash("redirected", "cat > src/novo.rs <<'EOF'\nfn novo() {}\nEOF".into()),
        bash("outside-script", format!("python3 {scratch}/edit.py")),
        bash("piped-program", "echo \"open('src/frete.rs', 'w').write('')\" | python3".into()),
        bash("launched", "uv run python -c \"open('src/frete.rs', 'w').write('')\"".into()),
        bash("wave-read", "mustard-rt run read request-3 | grep -n Tarefa".into()),
        bash("no-path", "python3 -c 'print(1)'".into()),
        bash("copied-out", format!("cat src/frete.rs > {scratch}/copia.rs")),
        bash("module", "python3 -m pytest".into()),
    ];
    let review: Vec<Call> = vec![
        bash("review-read", "mustard-rt run panel | jq .".into()),
        bash("review-edit", "sed -i 's/x/y/' src/frete.rs".into()),
    ];
    calls_conversation(&config, "loja/s1.jsonl", Some(&root), &conductor);
    let agents = config.join("projects").join("loja").join("s1").join("subagents");
    for (name, calls, meta) in [
        ("agent-w", &wave, json!({"agentType": "mustard:mustard-wave", "description": "# obra — onda 3"})),
        ("agent-r", &review, json!({"agentType": "mustard-review", "description": "# obra — revisão final"})),
    ] {
        calls_conversation(&config, &format!("loja/s1/subagents/{name}.jsonl"), Some(&root), calls);
        std::fs::write(agents.join(format!("{name}.meta.json")), meta.to_string()).unwrap();
    }
    calls_conversation(&config, "outra/s2.jsonl", Some(&other), &[bash("elsewhere", "mustard-rt run panel | jq .".into())]);

    let answer = measure(dir.path(), &config, &root, &["--since", "2026-10-01"]);
    let scripts = &answer["scripts"];
    let role = |around: u64, edits: u64| json!({"conversations": 1, "around": around, "edits": edits});
    let roles = [scripts["main"].clone(), scripts["wave"].clone(), scripts["other"].clone()];
    assert_eq!(roles, [role(11, 1), role(1, 6), role(1, 1)], "{answer}");
    let mut by: Vec<Value> = scripts["by_conversation"]
        .as_array()
        .unwrap()
        .iter()
        .map(|seen| json!([seen["conversation"], seen["role"], seen["description"], seen["around"], seen["edits"]]))
        .collect();
    by.sort_by_key(|seen| seen[0].as_str().unwrap().to_string());
    let expected = json!([
        ["loja/s1.jsonl", "main", null, 11, 1],
        ["loja/s1/subagents/agent-r.jsonl", "other", "# obra — revisão final", 1, 1],
        ["loja/s1/subagents/agent-w.jsonl", "wave", "# obra — onda 3", 1, 6],
    ]);
    assert_eq!(json!(by), expected, "{answer}");
    let table = [
        "| | condutor | agente de onda | outros agentes |",
        "|---|---:|---:|---:|",
        "| conversas | 1 | 1 | 1 |",
        "| scripts em volta do Mustard | 11 | 1 | 1 |",
        "| edições por script | 1 | 6 | 1 |",
    ];
    assert!(answer["text"].as_str().unwrap().ends_with(&format!("\n\n{}", table.join("\n"))), "{answer}");
}

/// A medição reconhece o `python` pela gramática do interpretador da recusa
/// de edição, e não por uma lista de nomes: o `python` rodado por lançador
/// (`uv run python`), o `py` e o `python.exe` leem a resposta do Mustard pelo
/// cano e montam os dados de uma gravação pela substituição e pelo arquivo do
/// `--json @arquivo`, como o `python3`. Ficam fora o programa do lançador que
/// não é interpretador e o `python` sem o Mustard.
#[test]
fn the_measure_reads_python_by_the_interpreter_grammar_of_the_edit_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    let root = project(&dir.path().join("loja"));
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
    let scratch = dir.path().join("scratch").display().to_string();
    let at = "2026-10-02T10:00:00Z";
    let bash = |id: &'static str, command: String| -> Call<'static> {
        (id, at, "Bash", json!({ "command": command }), None, String::new(), false)
    };
    let print = "-c 'import sys; print(sys.stdin.read())'";
    let calls: Vec<Call> = vec![
        bash("launched", format!("mustard-rt run read request-1 | uv run python {print}")),
        bash("windows", format!("mustard-rt run read request-1 | py -3 {print}")),
        bash("exe", format!("mustard-rt run read request-1 | python.exe {print}")),
        bash("substituted", "mustard-rt run write step --json \"$(uv run python -c 'print(1)')\"".into()),
        bash(
            "built",
            format!("uv run python -c \"import json; json.dump({{'wave': 1}}, open('{scratch}/step.json', 'w'))\""),
        ),
        bash("consumed", format!("mustard-rt run write step --json @{scratch}/step.json")),
        bash("other-program", "mustard-rt run read request-1 | uv run pytest".into()),
        bash("no-mustard", "uv run python -c 'print(1)'".into()),
    ];
    calls_conversation(&config, "loja/s1.jsonl", Some(&root), &calls);

    let answer = measure(dir.path(), &config, &root, &["--since", "2026-10-01"]);
    assert_eq!(answer["scripts"]["main"], json!({"conversations": 1, "around": 5, "edits": 0}), "{answer}");
}
