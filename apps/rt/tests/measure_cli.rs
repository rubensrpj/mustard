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
        "Nenhuma busca respondida desde a marca. Ainda não dá para dizer: faltam 200 para o mínimo de 200.",
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
        "No answered searches since the mark. Too early to tell: 200 more are needed to reach the minimum of 200.",
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
        "Nenhuma busca respondida desde a marca. Ainda não dá para dizer: faltam 200 para o mínimo de 200.",
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
        "Nenhuma busca respondida desde a marca. Ainda não dá para dizer: faltam 200 para o mínimo de 200.",
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
        calls_conversation(&config, "loja/s1.jsonl", Some(&root), &[
            ("early", "2026-09-30T10:00:00Z", "Bash", grep("grep -rn frete src"), Some(partial.clone()), found(), false),
            ("partial", at, "Bash", grep("cd src && grep -rn frete ."), Some(partial.clone()), found(), false),
            ("read", at, "Read", json!({"file_path": "src/frete.rs"}), Some(partial.clone()), found(), false),
            ("names", at, "Bash", grep("rg -l frete src | wc -l"), Some(names), "2".to_string(), false),
            ("failed", at, "Bash", grep("git grep frete"), Some(partial.clone()), error, true),
            ("silent", at, "Bash", grep("grep -rn frete src"), Some(partial.clone()), String::new(), false),
            ("not-found", at, "Bash", grep("grep -rn frete src"), Some(say("map.search.not_found")), found(), false),
        ]);
        calls_conversation(&config, "loja/s1/subagents/agent-a.jsonl", None, &[
            ("pinned", at, "Grep", json!({"pattern": "frete"}), None, pinned, true),
            ("next", at, "Read", json!({"file_path": "src/taxa.rs"}), None, found(), false),
        ]);
        let elsewhere = ("other", at, "Bash", grep("grep -rn frete src"), Some(partial.clone()), found(), false);
        calls_conversation(&config, "outra/s2.jsonl", Some(&other), &[elsewhere]);

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
        let counted = json!({
            "answered": 5, "pinned": 1, "partial": 3, "partial_unsure": 1, "failed": 2,
            "in_rate": 3, "used": 2, "opened_other": 0, "searched_again": 1, "moved_on": 0,
        });
        assert_eq!(answer["searches"], counted, "{code}: {answer}");
        let missing = translate("measure.searches_missing", lang).replace("{missing}", "197").replace("{min}", "200");
        assert!(answer["text"].as_str().unwrap().ends_with(&format!(" {missing}")), "{code}: {answer}");
    }
}

/// Cada busca respondida, numa conversa só dela, termina pela primeira das
/// chamadas seguintes que abre arquivo ou busca: ler, o resumo do mapa e o
/// `sed -i` abrem, com a posição do arquivo na lista; o arquivo fora da lista,
/// ou qualquer um quando a resposta não lista, é abriu outro; a nova busca é
/// buscou de novo; sem nenhuma, seguiu. A busca com `cd` numa cópia compara
/// os caminhos pela raiz da cópia. Falha a busca cujo curinga sem aspas o zsh
/// recusou e a que passa por um cano sem juntar as linhas e não mostra
/// arquivo; a que passa por um cano que junta as linhas vale.
#[test]
fn each_answered_search_ends_by_the_first_next_call_that_opens_or_searches() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    let root = project(&dir.path().join("loja"));
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
    let copy = dir.path().join("copias").join("w1");
    std::fs::create_dir_all(copy.join("src")).unwrap();
    std::fs::write(copy.join(".git"), "gitdir: elsewhere").unwrap();
    let lang = Locale::PtBr;
    let say = |key: &str| translate(key, lang).replace("{missing}", "`taxa`").replace("{words}", "\"frete\"");
    let partial = format!("{}\nsrc/frete.rs\nsrc/pedido.rs", say("map.answer.partial"));
    let unlisted = format!("{} {}", say("map.answer.partial_unsure"), say("map.answer.names_only"));
    let at = "2026-10-02T10:00:00Z";
    let bash = |command: &str| json!({ "command": command });
    let read = |path: &str| json!({ "file_path": path });
    let (in_root, in_copy) = (root.join("src/pedido.rs"), format!("cd {} && grep -rn frete .", copy.display()));
    let copied = copy.join("src/frete.rs");
    let found = "src/frete.rs:3:fn calcular";
    // Cada caso: a busca, a resposta do gancho, a saída e as chamadas seguintes.
    type Case<'a> = (&'a str, &'a str, &'a String, &'a str, Vec<(&'a str, Value)>);
    let cases: [Case; 11] = [
        ("second", "grep -rn frete src", &partial, found, vec![("Read", read(&in_root.to_string_lossy()))]),
        ("map", "grep -rn frete src", &partial, found, vec![("Bash", bash("mustard-rt run map summary --file src/frete.rs"))]),
        ("edit", "grep -rn frete src", &partial, found, vec![("Bash", bash("echo ok")), ("Bash", bash("sed -i s/a/b/ src/pedido.rs"))]),
        ("other", "grep -rn frete src", &partial, found, vec![("Read", read("src/outro.rs"))]),
        ("again", "grep -rn frete src", &partial, found, vec![("Bash", bash("ls")), ("Grep", json!({"pattern": "taxa"}))]),
        ("on", "grep -rn frete src", &partial, found, vec![("Bash", bash("echo a")), ("Bash", bash("echo b")), ("Bash", bash("echo c"))]),
        ("copy", &in_copy, &partial, found, vec![("Read", read(&copied.to_string_lossy()))]),
        ("unlisted", "grep -rln frete src", &unlisted, found, vec![("Read", read("src/frete.rs"))]),
        ("refused", "grep -rn frete src --include=*.rs", &partial, "(eval):1: no matches found: --include=*.rs", vec![]),
        ("counted", "grep -rn frete src | wc -l", &partial, "2", vec![("Read", read("src/frete.rs"))]),
        ("cut", "grep -rn frete src | head -3", &partial, "nada", vec![]),
    ];
    for (id, command, said, output, next) in &cases {
        let search = (*id, at, "Bash", bash(command), Some((*said).clone()), (*output).to_string(), false);
        let after = next.iter().map(|(name, input)| ("next", at, *name, input.clone(), None, found.to_string(), false));
        calls_conversation(&config, &format!("loja/{id}.jsonl"), Some(&root), &[vec![search], after.collect()].concat());
    }

    let lines = measure(dir.path(), &config, &root, &["--since", "2026-10-01", "--lines"]);
    let mut seen: Vec<Value> = lines.as_array().unwrap().iter().map(|line| {
        json!([line["tool_use_id"], line["outcome"], line["position"], line["failed"]])
    }).collect();
    seen.sort_by_key(|line| line[0].as_str().unwrap().to_string());
    let expected = json!([
        ["again", "searched_again", null, false],
        ["copy", "used", 1, false],
        ["counted", "used", 1, false],
        ["cut", "moved_on", null, true],
        ["edit", "used", 2, false],
        ["map", "used", 1, false],
        ["on", "moved_on", null, false],
        ["other", "opened_other", null, false],
        ["refused", "moved_on", null, true],
        ["second", "used", 2, false],
        ["unlisted", "opened_other", null, false],
    ]);
    assert_eq!(json!(seen), expected);
}

/// A tabela diz, de cada 100 buscas na taxa, quantas aproveitaram, abriram
/// outro arquivo e buscaram de novo, com a linha mais fraca em negrito, e a
/// frase diz quantas buscas, de que dia a que dia e se o número vale: com 200
/// buscas vale; com 3, faltam 197 para o mínimo.
#[test]
fn the_search_table_counts_each_hundred_and_says_whether_the_number_holds() {
    let dir = tempfile::tempdir().unwrap();
    let root = project(&dir.path().join("loja"));
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
    let say = |key: &str| translate(key, Locale::PtBr).replace("{missing}", "`taxa`").replace("{words}", "\"frete\"");
    let partial = format!("{}\nsrc/frete.rs", say("map.answer.partial"));
    // Grava, numa conversa, as buscas que aproveitam, que abrem outro e que
    // buscam de novo, cada uma seguida da chamada que a decide.
    let searches = |config: &Path, counts: [usize; 3]| {
        let ids: Vec<(String, String)> = (0..counts.iter().sum()).map(|n| (format!("s{n}"), format!("d{n}"))).collect();
        let decide = |n: usize| match n {
            n if n < counts[0] => ("Read", json!({"file_path": "src/frete.rs"})),
            n if n < counts[0] + counts[1] => ("Read", json!({"file_path": "src/outro.rs"})),
            _ => ("Grep", json!({"pattern": "taxa"})),
        };
        let calls: Vec<Call> = ids.iter().enumerate().flat_map(|(n, (search, decider))| {
            let at = if n == 0 { "2026-10-02T10:00:00Z" } else { "2026-10-03T10:00:00Z" };
            let (name, input) = decide(n);
            let command = json!({"command": "grep -rn frete src"});
            let found = "src/frete.rs:3:fn calcular".to_string();
            [(search.as_str(), at, "Bash", command, Some(partial.clone()), found.clone(), false), (decider.as_str(), at, name, input, None, found, false)]
        }).collect();
        calls_conversation(config, "loja/s1.jsonl", Some(&root), &calls);
        measure(dir.path(), config, &root, &["--since", "2026-10-01"])["text"].as_str().unwrap().to_string()
    };
    let head = "| Depois da resposta do Mustard, o Claude... | de cada 100 buscas |\n|---|---:|";
    let many = [
        head,
        "| abriu um arquivo que o Mustard listou | 55 |",
        "| abriu um arquivo fora da lista | 15 |",
        "| **buscou de novo** | **30** |",
        "",
        "200 buscas respondidas, de 02/10 a 03/10. O número vale: passou do mínimo de 200.",
    ];
    let text = searches(&dir.path().join("many"), [110, 30, 60]);
    assert!(text.ends_with(&format!("\n\n{}", many.join("\n"))), "{text}");
    let few = [
        head,
        "| abriu um arquivo que o Mustard listou | 33 |",
        "| **abriu um arquivo fora da lista** | **67** |",
        "| buscou de novo | 0 |",
        "",
        "3 buscas respondidas, de 02/10 a 03/10. Ainda não dá para dizer: faltam 197 para o mínimo de 200.",
    ];
    let text = searches(&dir.path().join("few"), [1, 2, 0]);
    assert!(text.ends_with(&format!("\n\n{}", few.join("\n"))), "{text}");
}
