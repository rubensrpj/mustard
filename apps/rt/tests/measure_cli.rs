// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A medição do uso real pelo binário de verdade: o comando `run measure`,
//! com as conversas do Claude Code, a pasta do gasto e a pasta pessoal numa
//! pasta temporária.

use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_core::platform::i18n::{translate, Locale};
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
/// de `config` e a pasta do gasto e a pessoal dentro de `dir`; com `--lines`,
/// a lista das linhas.
fn measure(dir: &Path, config: &Path, cwd: &Path, args: &[&str]) -> Value {
    let home = dir.join("home");
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args([&["run", "measure"], args].concat())
        .current_dir(cwd)
        .env("CLAUDE_PROJECT_DIR", cwd)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("CLAUDE_CONFIG_DIR", config)
        .env("MUSTARD_SPEND_DIR", dir.join("spend"))
        .env("MUSTARD_CLAUDE_BIN", home.join("no-claude-here"))
        .env_remove("CLAUDE_PLUGIN_ROOT")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .output()
        .expect("the binary runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let values = serde_json::Deserializer::from_str(&text).into_iter::<Value>().collect::<Result<Vec<_>, _>>();
    let mut values = values.unwrap_or_else(|e| panic!("not JSON ({e}): {text}"));
    if args.contains(&"--lines") { Value::Array(values) } else { values.pop().unwrap_or_else(|| panic!("no answer: {text}")) }
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

/// A resposta traz a tabela do gasto e a frase do veredito, no idioma do
/// projeto: dois dias de cada lado, como no modelo aprovado, ainda não dão
/// para dizer; cinco de cada lado dão; a marca de agora, sem dia depois, diz
/// que a versão ainda não foi usada, com o antes pronto. Sem marca, a medição
/// começa na próxima sessão; o instante que não se lê é recusado. Nada vai ao
/// arquivo do gasto nem à spec.
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
        "Buscas que o Mustard respondeu desde a marca: 0.",
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
        "Searches Mustard answered since the mark: 0.",
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
        "Buscas que o Mustard respondeu desde a marca: 0.",
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
    let not_used_yet = [
        "| | antes (29, 30/09, 01, 02 e 03/10) | depois |",
        "|---|---:|---:|",
        "| dias contados | 5 | 0 |",
        "| ações do Claude | 31.885 | sem uso |",
        "| tokens | 4.006 milhões | sem uso |",
        "| **tokens por ação** | **126 mil** | sem uso |",
        "",
        "Esta versão ainda não foi usada neste projeto. A coluna de antes já está pronta.",
        "",
        "Buscas que o Mustard respondeu desde a marca: 0.",
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

/// As buscas que o Mustard respondeu desde a marca, lidas das conversas do
/// projeto, da sessão e do agente dela, com a resposta reconhecida pelo
/// catálogo nos dois idiomas: a parcial, com os arquivos que ela lista; a
/// cravada, no lugar da busca; a que só marca a busca, sem lista; e a que deu
/// erro ou nenhuma saída, marcada à parte. Cada uma leva as três chamadas
/// seguintes da conversa. Ficam fora a busca antes da marca, a resposta de
/// que o mapa não achou, a chamada que não é busca e a conversa de outro
/// projeto.
#[test]
fn the_answered_searches_since_the_mark_come_from_the_project_conversations() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    let (root, other) = (project(&dir.path().join("loja")), project(&dir.path().join("outra")));
    let at = "2026-10-02T10:00:00Z";
    // Cada chamada: o código, o instante, a ferramenta, a entrada, o contexto
    // que o gancho pôs antes dela e o resultado, com a marca de erro.
    type Call = (&'static str, &'static str, &'static str, Value, Option<String>, String, bool);
    let write = |file: &str, cwd: Option<&Path>, calls: &[Call]| {
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
    };
    for (lang, code) in [(Locale::PtBr, "pt-BR"), (Locale::EnUs, "en-US")] {
        std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{code}"}}}}"#)).unwrap();
        let say = |key: &str| translate(key, lang).replace("{missing}", "`taxa`").replace("{words}", "\"frete\"");
        let changed = translate("map.answer.changed", lang);
        let partial = format!("{}\nsrc/frete.rs ({changed})\n  1-9 calcular (3)\nsrc/pedido.rs\n  4-8 fechar (5)", say("map.answer.partial"));
        let only = translate("map.answer.map_only", lang).replace("{files}", "`src/frete.rs`, `src/taxa.rs`");
        let pinned = format!("PreToolUse:Grep hook error: {}\n{only}", say("map.answer.pinned"));
        let names = format!("{} {}", say("map.answer.partial_unsure"), say("map.answer.names_only"));
        let grep = |command: &str| json!({ "command": command });
        let found = || "src/frete.rs:3:fn calcular".to_string();
        let error = "fatal: not a git repository".to_string();
        write("loja/s1.jsonl", Some(&root), &[
            ("early", "2026-09-30T10:00:00Z", "Bash", grep("grep -rn frete src"), Some(partial.clone()), found(), false),
            ("partial", at, "Bash", grep("cd src && grep -rn frete ."), Some(partial.clone()), found(), false),
            ("read", at, "Read", json!({"file_path": "src/frete.rs"}), Some(partial.clone()), found(), false),
            ("names", at, "Bash", grep("rg -l frete src | wc -l"), Some(names), "2".to_string(), false),
            ("failed", at, "Bash", grep("git grep frete"), Some(partial.clone()), error, true),
            ("silent", at, "Bash", grep("grep -rn frete src"), Some(partial.clone()), String::new(), false),
            ("not-found", at, "Bash", grep("grep -rn frete src"), Some(say("map.search.not_found")), found(), false),
        ]);
        write("loja/s1/subagents/agent-a.jsonl", None, &[
            ("pinned", at, "Grep", json!({"pattern": "frete"}), None, pinned, true),
            ("next", at, "Read", json!({"file_path": "src/taxa.rs"}), None, found(), false),
        ]);
        let elsewhere = ("other", at, "Bash", grep("grep -rn frete src"), Some(partial.clone()), found(), false);
        write("outra/s2.jsonl", Some(&other), &[elsewhere]);

        let since = ["--since", "2026-10-01"];
        let lines = measure(dir.path(), &config, &root, &[&since[..], &["--lines"]].concat());
        let seen: Vec<Value> = lines
            .as_array()
            .unwrap()
            .iter()
            .map(|line| {
                let next: Vec<Value> = line["next"].as_array().unwrap().iter().map(|call| call["tool"].clone()).collect();
                let kept = ["tool_use_id", "conversation", "at", "class", "files", "failed", "cwd"].map(|key| line[key].clone());
                json!([kept, next])
            })
            .collect();
        let shop = root.to_string_lossy();
        let (both, map_only) = (json!(["src/frete.rs", "src/pedido.rs"]), json!(["src/frete.rs", "src/taxa.rs"]));
        let expected = [
            json!([["partial", "loja/s1.jsonl", at, "partial", both, false, shop], ["Read", "Bash", "Bash"]]),
            json!([["names", "loja/s1.jsonl", at, "partial_unsure", [], false, shop], ["Bash", "Bash", "Bash"]]),
            json!([["failed", "loja/s1.jsonl", at, "partial", both, true, shop], ["Bash", "Bash"]]),
            json!([["silent", "loja/s1.jsonl", at, "partial", both, true, shop], ["Bash"]]),
            json!([["pinned", "loja/s1/subagents/agent-a.jsonl", at, "pinned", map_only, false, ""], ["Read"]]),
        ];
        assert_eq!(seen, expected, "{code}");
        let answer = measure(dir.path(), &config, &root, &since);
        let counted = json!({"answered": 5, "pinned": 1, "partial": 3, "partial_unsure": 1, "failed": 2});
        assert_eq!(answer["searches"], counted, "{code}: {answer}");
        let sentence = translate("measure.searches", lang).replace("{count}", "5");
        assert!(answer["text"].as_str().unwrap().ends_with(&format!("\n\n{sentence}")), "{code}: {answer}");
    }
}
