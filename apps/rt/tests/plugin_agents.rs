// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Os textos de agente do Mustard, pelo binário de verdade.
//!
//! O projeto recebe exatamente dois agentes — `mustard-wave` e
//! `mustard-review` —, no idioma do
//! `language.text`; os dois idiomas existem como molde do produto; nenhum
//! texto manda criar cópia do projeto por conta própria, e os de onda e de
//! revisão mandam trabalhar na cópia e na pasta de compilação que o pedido
//! indica; e cada comando do fluxo responde o próximo passo, que o modelo não
//! escolhe sozinho. O que prende o texto de um agente é o que ele diz, não
//! quantos bytes ele tem. O agente de onda não traz teto de idas e
//! voltas no cabeçalho: os tetos de dez e quinze cortavam toda onda real no
//! meio, e a onda cortada recomeçava do zero.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

/// O jeito de o agente criar uma cópia do projeto por conta própria: a cópia
/// em si, a pasta de compilação escolhida por ele, a pasta compartilhada que
/// só servia às cópias soltas, e a porta que apagava a cópia depois. Só o
/// pedido que o binário monta diz em que cópia e em que pasta trabalhar.
const COPY_ON_ITS_OWN: &[&str] = &[
    "CARGO_TARGET_DIR",
    "scratch-target",
    "cp -r",
    "cp -R",
    "cp -a",
    "rsync",
    "git clone",
    "git worktree",
    "worktree",
    "EnterWorktree",
    "clean --path",
];

fn repo_root() -> PathBuf {
    manifest_dir::manifest_dir().join("../..")
}

/// A pasta-base das cópias que o binário cria num teste: dentro da pasta
/// pessoal falsa dele, e por isso dentro da pasta temporária, que a leva
/// quando sai. O binário a recebe pela variável do ambiente, sempre, para o
/// teste e ele concordarem, qualquer que seja o ambiente de quem roda.
fn copies_base(home: &Path) -> PathBuf {
    home.join("copias")
}

/// A pasta das cópias do projeto `root` sob a base do teste: o nome da pasta
/// do projeto sob a base é o que o binário monta, e não depende da base.
fn copies_of(home: &Path, root: &Path) -> PathBuf {
    copies_base(home).join(mustard_core::io::wave_prompt::copies_dir(root).file_name().unwrap())
}

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git").args(args).current_dir(root).output().map(|o| o.status.success()).unwrap_or(false);
    assert!(ok, "git {args:?} failed");
}

/// Roda o binário no projeto, com uma pasta pessoal falsa e sem `claude` à mão.
fn rt(root: &Path, home: &Path, args: &[&str], stdin: Option<&str>) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(args)
        .current_dir(root)
        .env("CLAUDE_PROJECT_DIR", root)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("MUSTARD_COPIES_DIR", copies_base(home))
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CLAUDE_PLUGIN_ROOT")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .env("MUSTARD_CLAUDE_BIN", home.join("no-claude-here"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(stdin.unwrap_or_default().as_bytes());
    }
    let out = child.wait_with_output().expect("the binary finishes");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}{}", String::from_utf8_lossy(&out.stderr)))
}

/// O pedido de um item que a rodada despachou, lido como o agente o lê: pelo
/// comando que a resposta traz no lugar do pedido, rodado pelo binário, com a
/// saída crua.
fn request_by_command(root: &Path, home: &Path, entry: &Value) -> String {
    let command = entry["read"].as_str().unwrap_or_else(|| panic!("the dispatch carries no read command: {entry}"));
    let words: Vec<&str> = command.split_whitespace().collect();
    assert_eq!(words.first(), Some(&"mustard-rt"), "{command}");
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(&words[1..])
        .current_dir(root)
        .env("HOME", home)
        .env("MUSTARD_COPIES_DIR", copies_base(home))
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .output()
        .expect("the binary runs");
    assert!(out.status.success(), "{command}: {}", String::from_utf8_lossy(&out.stdout));
    String::from_utf8(out.stdout).expect("the request is text")
}

/// Um repositório com `main` e `dev`, parado em `dev`, com o `mustard.json`
/// dado e a instalação feita. Devolve a pasta do projeto e a pessoal falsa.
fn installed(dir: &Path, config: &str) -> (PathBuf, PathBuf) {
    let root = dir.join("project");
    let home = dir.join("home");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["checkout", "-q", "-b", "main"]);
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    git(&root, &["add", "src/main.rs"]);
    git(&root, &["commit", "-q", "-m", "init"]);
    git(&root, &["checkout", "-q", "-b", "dev"]);
    std::fs::write(root.join("mustard.json"), config).unwrap();
    let report = rt(&root, &home, &["run", "upsert"], None);
    assert!(report.get("error").is_none(), "{report}");
    (root, home)
}

/// Todo arquivo debaixo de `dir`, com o caminho a partir dele.
fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

fn template(lang: &str, name: &str) -> String {
    std::fs::read_to_string(repo_root().join(format!("packages/core/templates/agents/{lang}/{name}.md")))
        .unwrap_or_else(|e| panic!("the {lang} `{name}` template is missing: {e}"))
}

/// O projeto recebe exatamente os dois agentes, no idioma do
/// `language.text`; os dois idiomas existem como molde; e o
/// plugin não entrega agente nenhum, porque entregaria os dois idiomas. Os
/// textos que o instalador escreve trazem as duas guardas desta obra: provar
/// que nada se perde antes de apagar ou mover alguma coisa no git, e o teste
/// do caso em que o "antes" falha quando o critério diz "só depois de".
#[test]
fn the_project_receives_exactly_two_agents_in_its_text_language() {
    for (lang, other) in [("pt-BR", "en-US"), ("en-US", "pt-BR")] {
        let dir = tempfile::tempdir().unwrap();
        let (root, _home) = installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));

        let agents = files_under(&root.join(".claude/agents"));
        assert_eq!(
            agents,
            ["mustard/review.md", "mustard/wave.md"],
            "the {lang} project got another set of agent texts",
        );
        for name in ["wave", "review"] {
            let installed = std::fs::read_to_string(root.join(format!(".claude/agents/mustard/{name}.md"))).unwrap();
            assert_eq!(installed, template(lang, name), "the {lang} project got another text for `{name}`");
            assert_ne!(installed, template(other, name), "the {lang} and {other} `{name}` texts are the same");
            assert!(
                installed.starts_with(&format!("---\nname: mustard-{name}\n")),
                "the `{name}` file does not declare the agent `mustard-{name}`",
            );
        }

        // As duas guardas que esta obra aprendeu viajam com o produto: quem
        // apaga ou move alguma coisa no git prova antes que nada se perde, e
        // o critério que diz "só depois de" ganha o teste do caso em que o
        // "antes" falha. O agente de onda as cumpre; o revisor as confere.
        let guards: [(&str, [&str; 2]); 2] = if lang == "pt-BR" {
            [
                ("wave", [
                    "apagar ou mover algo no git, prove que nada se perde",
                    "\"só depois de\" ganha também o teste do caso em que o \"antes\" falha",
                ]),
                ("review", [
                    "apagou ou moveu algo no git? Confira que nada se perdeu",
                    "\"só depois de\" tem teste do caso em que o \"antes\" falha",
                ]),
            ]
        } else {
            [
                ("wave", [
                    "deleting or moving anything in git, prove nothing is lost",
                    "\"only after\" also gets a test of the case where the \"before\" fails",
                ]),
                ("review", [
                    "delete or move anything in git? Check that nothing was lost",
                    "\"only after\" has a test of the case where the \"before\" fails",
                ]),
            ]
        };
        for (name, lines) in guards {
            let installed = std::fs::read_to_string(root.join(format!(".claude/agents/mustard/{name}.md"))).unwrap();
            for line in lines {
                assert!(installed.contains(line), "the {lang} `{name}` agent does not say `{line}`");
            }
        }
    }
    assert!(!repo_root().join("plugin/agents").exists(), "the plugin ships agent texts of its own");
}

/// O cabeçalho de um molde de agente: o que está entre as duas linhas de três
/// traços no começo do arquivo, que é onde a plataforma lê os campos dele.
fn frontmatter(body: &str) -> &str {
    body.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---")).map(|(head, _)| head).unwrap_or(body)
}

/// O instalador escreve o molde do agente de onda (`wave.md`), que recebe
/// toda onda, de uma tarefa ou de várias, e ele não traz teto de idas e
/// voltas no cabeçalho. A medição das vinte e cinco
/// ondas entregues desta obra deu gasto de 36 a 403 idas, com média de 153:
/// nenhuma onda real cabia nos tetos de dez e quinze que havia aqui, e a onda
/// cortada recomeçava do zero. Quem cuida da janela cheia é a compactação e
/// quem cuida da onda parada é o sinal de vida da rodada. Ele não pede mais
/// um relatório pelo tamanho: a entrega vai gravada na spec.
#[test]
fn wave_agent_template_has_no_cap_on_round_trips() {
    for (lang, tokens) in [("pt-BR", "mil e dois mil tokens"), ("en-US", "one and two thousand tokens")] {
        let dir = tempfile::tempdir().unwrap();
        let (root, _home) = installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));

        let name = "wave";
        let path = root.join(format!(".claude/agents/mustard/{name}.md"));
        assert!(path.is_file(), "the {lang} installation did not write the {name} agent");
        let body = std::fs::read_to_string(&path).unwrap();
        let head = frontmatter(&body);
        let cap = head.lines().find(|line| line.to_lowercase().contains("turn"));
        assert!(cap.is_none(), "the {lang} {name} agent header still carries a turn cap: {cap:?}");
        assert!(!head.contains("10") && !head.contains("15"), "the {lang} {name} agent header still pins ten or fifteen: {head}");
        assert!(!body.contains(tokens), "the {lang} {name} agent still asks for a report by size");
    }
}

/// O nome de cada agente do Mustard leva o prefixo do Mustard, e um projeto
/// que já tem um agente chamado `review` fica com os dois: o dele, intocado,
/// e o `mustard-review`. Uma instalação antiga, com os nomes sem prefixo,
/// com o agente de onda de tarefa única e com o que escrevia skills, é
/// migrada pela instalação seguinte, que tira esses dois agentes e diz isso,
/// e a rodada manda cada pedido ao agente pelo nome com prefixo.
#[test]
fn the_mustard_agents_carry_the_prefix_and_live_beside_a_project_agent_of_the_same_name() {
    let dir = tempfile::tempdir().unwrap();
    let (root, home) = installed(dir.path(), r#"{"version":"1.0.0","language":{"text":"pt-BR"}}"#);
    let own = "---\nname: review\ndescription: O revisor do próprio projeto.\n---\n\nRevise.\n";
    std::fs::write(root.join(".claude/agents/review.md"), own).unwrap();
    // A instalação antiga: os dois agentes do Mustard sem o prefixo.
    for name in ["wave", "review"] {
        let path = root.join(format!(".claude/agents/mustard/{name}.md"));
        let old = std::fs::read_to_string(&path).unwrap().replacen(&format!("name: mustard-{name}"), &format!("name: {name}"), 1);
        std::fs::write(&path, old).unwrap();
    }
    // …o agente de onda de tarefa única, que foi juntado ao de onda, e o que
    // escrevia skills, cuja receita o programa monta sozinho.
    for retired in ["wave-solo", "skill"] {
        std::fs::write(
            root.join(format!(".claude/agents/mustard/{retired}.md")),
            format!("---\nname: {retired}\n---\n\nO molde antigo.\n"),
        )
        .unwrap();
    }

    let report = rt(&root, &home, &["run", "upsert"], None);
    assert!(report.get("error").is_none(), "{report}");
    for retired in ["wave-solo", "skill"] {
        assert!(!root.join(format!(".claude/agents/mustard/{retired}.md")).exists(), "the retired `{retired}` agent stayed: {report}");
        assert!(
            report.to_string().contains(&format!(".claude/agents/mustard/{retired}.md (retired agent)")),
            "the update does not say it took out `{retired}`: {report}",
        );
    }

    assert_eq!(std::fs::read_to_string(root.join(".claude/agents/review.md")).unwrap(), own, "the project's agent changed");
    let mut names: Vec<String> = files_under(&root.join(".claude/agents"))
        .iter()
        .filter_map(|file| {
            let body = std::fs::read_to_string(root.join(".claude/agents").join(file)).ok()?;
            body.lines().find_map(|line| line.strip_prefix("name: ")).map(str::to_string)
        })
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["mustard-review", "mustard-wave", "review"],
        "two agents share a name",
    );

    for lang in [Locale::PtBr, Locale::EnUs] {
        let next = translate("round.next", lang);
        assert!(next.contains("`mustard-wave`") && next.contains("`mustard-review`"), "{next}");
    }
}

/// O agente de onda instalado proíbe usar o stash do git, na mesma frase que
/// já proíbe comitar, enviar ao servidor e trocar de branch, nos dois
/// idiomas.
#[test]
fn the_wave_agent_never_uses_the_git_stash() {
    for (lang, phrase) in [("pt-BR", "use o stash"), ("en-US", "or stash")] {
        let dir = tempfile::tempdir().unwrap();
        let (root, _home) = installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));
        let wave = std::fs::read_to_string(root.join(".claude/agents/mustard/wave.md")).unwrap();
        let guard = wave
            .lines()
            .find(|l| l.contains("`git add`"))
            .unwrap_or_else(|| panic!("the {lang} wave agent lost its git guard line"));
        assert!(guard.contains(phrase), "the {lang} wave agent does not forbid the stash: {guard}");
    }
}

/// O molde da onda, nos dois idiomas, diz o que `met:true` quer dizer para o
/// item combinado que nenhuma tarefa da onda faz e que só vale para os
/// arquivos dela: que ele continua valendo depois da mudança. O `met:false`
/// fica para a mudança que o quebra e para a tarefa que o faz e ficou por
/// fazer, e o item não cumprido segue virando tarefa no backlog, ou entra na
/// tarefa que a onda deixou por fazer.
#[test]
fn the_wave_agent_calls_an_agreed_item_met_when_it_still_holds_after_the_change() {
    let said = [
        (
            "pt-BR",
            [
                "Para o item que nenhuma tarefa da onda faz e que só vale para os arquivos dela",
                "`met:true` quer dizer que ele continua valendo depois da sua mudança",
                "`met:false` só quando a mudança o quebra ou quando a tarefa que o faz ficou por fazer",
                "vira tarefa no backlog",
                "ou entra na de `undone`",
            ],
        ),
        (
            "en-US",
            [
                "For an item no task of the wave does and that only holds for its files",
                "`met:true` means it still holds after your change",
                "`met:false` only when the change undoes it or when the task that does it was not done",
                "becomes a backlog task",
                "or joins the one in `undone`",
            ],
        ),
    ];
    for (lang, phrases) in said {
        let wave = template(lang, "wave");
        let rule = wave
            .lines()
            .find(|line| line.contains("\"agreed\":[{"))
            .unwrap_or_else(|| panic!("the {lang} wave agent lost its agreed line"));
        for phrase in phrases {
            assert!(rule.contains(phrase), "the {lang} agreed line does not say `{phrase}`: {rule}");
        }
    }
}

/// O molde da onda, nos dois idiomas, diz na orientação sobre ferramentas que
/// o passo de término de cada tarefa leva o código dela e responde, com a
/// marca do Mustard, se o agente segue ou entrega, e que a resposta se
/// obedece; que tarefa começada se conclui antes da entrega; e não manda mais
/// parar no meio da tarefa, nem conta chamadas de folga.
#[test]
fn the_wave_agent_reads_the_size_at_each_task_end_and_finishes_what_it_started() {
    let said = [
        (
            "pt-BR",
            "## Orientação sobre ferramentas",
            [
                "ao terminar tarefa, com o código dela no `item`",
                "O resultado do passo de término traz, com a marca [Mustard], se você segue ou entrega",
                "o texto não é da ferramenta, e você o obedece",
                "Tarefa começada se conclui antes da entrega",
            ],
            ["Pare:", "8 chamadas", "150 mil"],
        ),
        (
            "en-US",
            "## Tool guidance",
            [
                "on finishing a task, with its code in `item`",
                "The finishing step's result says, marked [Mustard], whether you go on or deliver",
                "it is not the tool's text, so obey it",
                "A started task is finished before delivering",
            ],
            ["Stop:", "8 calls", "150 thousand"],
        ),
    ];
    for (lang, header, phrases, gone) in said {
        let wave = template(lang, "wave");
        let rule = section(&wave, header);
        for phrase in phrases {
            assert!(rule.contains(phrase), "the {lang} task end rule does not say `{phrase}`: {rule}");
        }
        for phrase in gone {
            assert!(!wave.contains(phrase), "the {lang} wave agent still stops mid-task: `{phrase}`");
        }
    }
}

/// Os dois agentes que gravam a própria volta dizem que a gravação é
/// obrigatória: sem isso um relatório em prosa vira entrega perdida, e a
/// rodada não acha nada na spec. Os dois também dizem que a lista de
/// pendências não é deles para fechar. Nos dois idiomas.
#[test]
fn the_agents_state_that_the_marked_line_is_mandatory_and_the_ledger_is_not_theirs() {
    for lang in ["pt-BR", "en-US"] {
        let dir = tempfile::tempdir().unwrap();
        let (root, _home) = installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));
        for name in ["wave", "review"] {
            let text = std::fs::read_to_string(root.join(format!(".claude/agents/mustard/{name}.md"))).unwrap();
            let mandatory = if lang == "pt-BR" { "obrigatória" } else { "mandatory" };
            assert!(text.contains(mandatory), "the {lang} `{name}` agent does not call its line mandatory");
            assert!(
                text.contains(".claude/pending/"),
                "the {lang} `{name}` agent does not say the pending ledger is not its to close"
            );
        }
    }
}

/// Os dois moldes — onda e revisão — declaram o modelo e o esforço padrão da
/// configuração, nos dois idiomas: cada agente sabe o próprio modelo e o
/// próprio esforço, sem herdar o da sessão em silêncio. O modelo e o esforço
/// do molde são os mesmos padrões que a instalação escreve no
/// `mustard.json`, para o molde copiado sem instalação valer o mesmo. Teto de
/// idas e voltas não anda junto: o molde de onda não traz um, e quem prende
/// isso é o teste do teto. Nenhum teste desta obra recusa um molde pelo
/// tamanho em bytes — o que prende o texto é o que ele diz.
#[test]
fn each_agent_template_declares_its_own_model_and_effort() {
    let default = mustard_core::domain::config::DEFAULT_AGENT_MODEL;
    let effort = mustard_core::domain::config::DEFAULT_AGENT_EFFORT;
    for lang in ["pt-BR", "en-US"] {
        for name in ["wave", "review"] {
            let text = template(lang, name);
            assert!(
                text.contains(&format!("\nmodel: {default}\n")),
                "the {lang} `{name}` agent does not declare the default model {default}:\n{text}",
            );
            assert!(
                text.contains(&format!("\neffort: {effort}\n")),
                "the {lang} `{name}` agent does not declare the default effort {effort}:\n{text}",
            );
            assert!(!text.contains("model: inherit"), "the {lang} `{name}` agent still inherits the session's model");
            assert!(!text.contains("model: opus"), "the {lang} `{name}` agent still fixes the opus model:\n{text}");
        }

        let wave = template(lang, "wave");
        assert!(wave.len() as u64 > 3_072, "the {lang} wave agent is not over the old byte cap, so it proves nothing");
    }
}

/// O modelo dos agentes vem do `mustard.json` do projeto, pelo instalador de
/// verdade: sem o campo, os dois agentes saem no padrão e o arquivo ganha o
/// campo; com `opus` no arquivo, os dois saem em opus e o campo fica onde
/// estava; trocada a linha, a instalação seguinte troca os dois agentes.
#[test]
fn the_installed_agents_use_the_model_of_the_project_config() {
    let model_of = |root: &Path, name: &str| -> String {
        let text = std::fs::read_to_string(root.join(format!(".claude/agents/mustard/{name}.md"))).unwrap();
        frontmatter(&text).lines().find_map(|line| line.strip_prefix("model: ")).unwrap_or_default().to_string()
    };
    let declared = |root: &Path| -> Value {
        let raw = std::fs::read_to_string(root.join("mustard.json")).unwrap();
        serde_json::from_str::<Value>(&raw).unwrap()["agents"].clone()
    };
    for lang in ["pt-BR", "en-US"] {
        let dir = tempfile::tempdir().unwrap();
        let (root, home) = installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));
        assert_eq!(declared(&root)["model"], json!("sonnet"), "the install did not write the default model");
        for name in ["wave", "review"] {
            assert_eq!(model_of(&root, name), "sonnet", "the {lang} `{name}` agent");
        }

        std::fs::write(
            root.join("mustard.json"),
            format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}},"agents":{{"model":"opus"}}}}"#),
        )
        .unwrap();
        let report = rt(&root, &home, &["run", "upsert"], None);
        assert!(report.get("error").is_none(), "{report}");
        assert_eq!(declared(&root)["model"], json!("opus"), "the install rewrote the model the person chose");
        for name in ["wave", "review"] {
            assert_eq!(model_of(&root, name), "opus", "the {lang} `{name}` agent kept the mold's model");
            let installed = std::fs::read_to_string(root.join(format!(".claude/agents/mustard/{name}.md"))).unwrap();
            assert_eq!(
                installed,
                template(lang, name).replacen("\nmodel: sonnet\n", "\nmodel: opus\n", 1),
                "the {lang} `{name}` agent changed more than its model line",
            );
        }
    }
}

/// O esforço dos agentes vem do `mustard.json` do projeto, pelo instalador
/// de verdade: sem o campo, os dois agentes saem no padrão e o arquivo ganha o
/// campo; com `medium` no arquivo, os dois saem em medium e o campo fica onde
/// estava; um valor fora da lista do Claude Code fica no arquivo e os agentes
/// voltam ao padrão; em todos os casos só a linha do esforço muda no agente.
#[test]
fn the_installed_agents_use_the_effort_of_the_project_config() {
    let effort_of = |root: &Path, name: &str| -> String {
        let text = std::fs::read_to_string(root.join(format!(".claude/agents/mustard/{name}.md"))).unwrap();
        frontmatter(&text).lines().find_map(|line| line.strip_prefix("effort: ")).unwrap_or_default().to_string()
    };
    let declared = |root: &Path| -> Value {
        let raw = std::fs::read_to_string(root.join("mustard.json")).unwrap();
        serde_json::from_str::<Value>(&raw).unwrap()["agents"].clone()
    };
    for lang in ["pt-BR", "en-US"] {
        let dir = tempfile::tempdir().unwrap();
        let (root, home) = installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));
        assert_eq!(declared(&root), json!({"model": "sonnet", "effort": "xhigh"}), "the install did not write the defaults");
        for name in ["wave", "review"] {
            assert_eq!(effort_of(&root, name), "xhigh", "the {lang} `{name}` agent");
        }

        for (written, expected) in [("medium", "medium"), ("max", "max"), ("ultra", "xhigh")] {
            std::fs::write(
                root.join("mustard.json"),
                format!(
                    r#"{{"version":"1.0.0","language":{{"text":"{lang}"}},"agents":{{"model":"sonnet","effort":"{written}"}}}}"#
                ),
            )
            .unwrap();
            let report = rt(&root, &home, &["run", "upsert"], None);
            assert!(report.get("error").is_none(), "{report}");
            assert_eq!(declared(&root)["effort"], json!(written), "the install rewrote the effort the person chose");
            for name in ["wave", "review"] {
                assert_eq!(effort_of(&root, name), expected, "the {lang} `{name}` agent with `{written}`");
                let installed = std::fs::read_to_string(root.join(format!(".claude/agents/mustard/{name}.md"))).unwrap();
                assert_eq!(
                    installed,
                    template(lang, name).replacen("\neffort: xhigh\n", &format!("\neffort: {expected}\n"), 1),
                    "the {lang} `{name}` agent changed more than its effort line",
                );
            }
        }
    }
}

/// O molde de onda, nos dois idiomas, não manda o agente refazer o que o
/// pedido já traz: o código parecido e o padrão do projeto vêm no pedido,
/// então `run map examples` e `run map importers` custariam duas chamadas, e
/// cada uma relê a conversa inteira, para devolver o que ele já leu; e os
/// idiomas do texto e do código vêm no cabeçalho do pedido, sem o molde
/// repeti-los. O molde manda seguir o código parecido que o pedido mostra.
#[test]
fn wave_template_does_not_say_to_redo_what_the_request_already_carries() {
    for (lang, said, repeated) in [
        (
            "pt-BR",
            "siga o código parecido que o pedido mostra, ou o arquivo vizinho",
            ["map examples", "map importers", "idiomas do cabeçalho"],
        ),
        (
            "en-US",
            "follow the similar code the request shows, or the neighboring file",
            ["map examples", "map importers", "languages in the request's header"],
        ),
    ] {
        let wave = template(lang, "wave");
        assert!(wave.contains(said), "the {lang} wave agent lost `{said}`");
        for phrase in repeated {
            assert!(!wave.contains(phrase), "the {lang} wave agent repeats what the request already carries: `{phrase}`");
        }
    }
}

/// O molde da onda, nos dois idiomas, tem exatamente quatro itens de
/// segundo nível — objetivo, orientação sobre ferramentas, fronteira da
/// tarefa e formato de saída, nas palavras da documentação da Anthropic —, e
/// nada além deles: a contagem de `## ` é quatro, sem um quinto item.
#[test]
fn the_wave_agent_template_has_exactly_four_items() {
    let headers: [(&str, [&str; 4]); 2] = [
        ("pt-BR", ["## Objetivo", "## Orientação sobre ferramentas", "## Fronteira da tarefa", "## Formato de saída"]),
        ("en-US", ["## Goal", "## Tool guidance", "## Task boundary", "## Output format"]),
    ];
    for (lang, items) in headers {
        let wave = template(lang, "wave");
        let found: Vec<&str> = wave.lines().filter(|line| line.starts_with("## ")).collect();
        assert_eq!(found, items, "the {lang} wave agent does not have exactly these four items: {wave}");
    }
}

/// O texto de uma seção de segundo nível de um molde: da linha `header` até a
/// próxima seção, sem a própria linha do título.
fn section<'a>(body: &'a str, header: &str) -> &'a str {
    let start = body.find(&format!("\n{header}\n")).unwrap_or_else(|| panic!("no `{header}` section in:\n{body}"));
    let rest = &body[start + header.len() + 2..];
    rest.find("\n## ").map_or(rest, |end| &rest[..end])
}

/// A orientação sobre ferramentas do molde da onda, nos dois idiomas, pede
/// num item só que as leituras que não dependem uma da outra — ler, buscar,
/// listar, ler a spec pelo binário ou ler pelo terminal — saiam juntas: em
/// várias chamadas numa resposta ou em vários trechos num comando só do
/// terminal, porque cada resposta relê a conversa inteira do agente.
#[test]
fn the_wave_agent_asks_for_independent_reads_together_in_one_response() {
    for (lang, header, said) in [
        (
            "pt-BR",
            "## Orientação sobre ferramentas",
            [
                "não dependem uma da outra",
                "várias chamadas numa resposta",
                "(Read, Grep, Glob, `mustard-rt run read` ou o terminal)",
                "vários trechos num comando só do terminal",
                "relê a conversa inteira",
            ],
        ),
        (
            "en-US",
            "## Tool guidance",
            [
                "do not depend on each other",
                "several calls in one response",
                "(Read, Grep, Glob, `mustard-rt run read` or the terminal)",
                "several excerpts in a single terminal command",
                "rereads the whole conversation",
            ],
        ),
    ] {
        let body = template(lang, "wave");
        let tools = section(&body, header);
        let item = tools
            .lines()
            .find(|line| line.starts_with("- ") && line.contains(said[0]))
            .unwrap_or_else(|| panic!("the {lang} wave tool guidance does not ask for independent calls together:{tools}"));
        for phrase in said {
            assert!(item.contains(phrase), "the {lang} item on independent calls does not say `{phrase}`: {item}");
        }
    }
}

/// Os títulos da fronteira da tarefa e do formato de saída, por idioma.
fn wave_headers(lang: &str) -> (&'static str, &'static str) {
    if lang == "pt-BR" { ("## Fronteira da tarefa", "## Formato de saída") } else { ("## Task boundary", "## Output format") }
}

/// O molde do agente de onda, nos dois idiomas, manda mudar o arquivo
/// que a mesma mudança exige, mesmo fora da lista da tarefa, e dizê-lo em
/// `files`; a mudança de plano fica para o critério que precisa mudar ou a
/// spec que não diz o que fazer, e sai ao perceber, antes de explorar o
/// resto. A tarefa do pedido que o agente não fez vai na lista das não
/// feitas, com ou sem mudança de plano. A fronteira não manda mais parar
/// porque falta arquivo na lista.
#[test]
fn wave_template_puts_in_the_task_the_file_the_change_requires() {
    for (lang, said, gone) in [
        ("pt-BR", ["entra no trabalho", "antes de explorar", "`files`", "`replan`", "`undone`"], "arquivo que falta"),
        ("en-US", ["is part of the work", "before exploring", "`files`", "`replan`", "`undone`"], "a missing file"),
    ] {
        let name = "wave";
        let body = template(lang, name);
        let boundary = section(&body, wave_headers(lang).0);
        for phrase in said {
            assert!(boundary.contains(phrase), "the {lang} `{name}` boundary does not say `{phrase}`:{boundary}");
        }
        assert!(!body.contains(gone), "the {lang} `{name}` agent still stops for `{gone}`:{boundary}");
    }
}

/// A fronteira do molde de onda, nos dois idiomas, manda tirar na
/// mesma onda o que a própria mudança deixou sem uso, com o teste que só
/// existia para ele; num arquivo de outra onda em andamento, o agente não
/// edita e deixa a sobra em `leftovers`, o campo da entrega que a rodada
/// grava, só com o título e o detalhe: toda sobra vai ao backlog da spec, e o
/// molde não pede mais que o agente diga se ela quebra algo ou é cosmética. A
/// sobra que só muda comentário, documentação ou texto de ajuda leva a marca
/// de limpeza, que a rodada segura para o fim da obra.
#[test]
fn boundary_says_to_remove_what_the_change_left_unused() {
    for (lang, said) in [
        (
            "pt-BR",
            [
                "deixa sem uso",
                "com o teste só dele",
                "sai na mesma onda",
                "outra onda em andamento, não edite",
                "backlog da spec",
                "só muda comentário, documentação ou texto de ajuda",
                "`\"cleanup\":true`",
            ],
        ),
        (
            "en-US",
            [
                "leaves unused",
                "with the test only it had",
                "goes in the same wave",
                "another running wave, do not edit",
                "spec backlog",
                "only changes a comment, documentation or help text",
                "`\"cleanup\":true`",
            ],
        ),
    ] {
        let name = "wave";
        let body = template(lang, name);
        let boundary = section(&body, wave_headers(lang).0);
        let field = "`\"leftovers\":[{\"title\":\"…\",\"detail\":\"…\"}]`";
        for phrase in said.iter().chain(std::iter::once(&field)) {
            assert!(boundary.contains(phrase), "the {lang} `{name}` boundary does not say `{phrase}`:{boundary}");
        }
        for gone in ["\"kind\"", "`kind`", "breaks", "cosmetic"] {
            assert!(!body.contains(gone), "the {lang} `{name}` agent still sorts the leftover by `{gone}`:{boundary}");
        }
    }
}

/// Os moldes dizem o limite do que entra. O agente de onda põe só o que tem
/// uso fora de teste, um teste por comportamento e nenhum código que só serve
/// a medição, e a tarefa que só tira código, junta testes ou muda
/// configuração se prova pela suíte, sem teste novo. O revisor tem por Maior
/// o teste repetido e o código de laboratório no programa instalado.
#[test]
fn templates_state_the_size_limit_and_when_no_new_test_is_needed() {
    for (lang, tools, boundary, severity, wave_said, review_said) in [
        (
            "pt-BR",
            "## Orientação sobre ferramentas",
            "## Fronteira da tarefa",
            "## Gravidade",
            [
                "Critério que muda comportamento ganha um teste",
                "Tarefa que só tira código, junta testes ou muda configuração prova pela suíte",
                "sem teste novo nem leitor de configuração",
                "tem uso fora de teste; um teste por comportamento, sem repetir outro",
                "nada de código só de medição",
            ],
            [
                "repete outro teste",
                "código só de laboratório no programa instalado",
            ],
        ),
        (
            "en-US",
            "## Tool guidance",
            "## Task boundary",
            "## Severity",
            [
                "A criterion that changes behavior gets a test",
                "A task that only removes code, merges tests or changes configuration is proved by the suite",
                "no new test and no configuration reader",
                "has a use outside tests; one test per behavior, never repeating another",
                "no measurement-only code",
            ],
            [
                "repeats another test",
                "laboratory-only code in the installed program",
            ],
        ),
    ] {
        let wave = template(lang, "wave");
        let said = section(&wave, tools).to_string() + section(&wave, boundary);
        for phrase in wave_said {
            assert!(said.contains(phrase), "the {lang} wave agent does not say `{phrase}`:{said}");
        }
        let review = template(lang, "review");
        let major = section(&review, severity);
        for phrase in review_said {
            assert!(major.contains(phrase), "the {lang} reviewer does not call `{phrase}` major:{major}");
        }
    }
}

/// Todo comando de compilação ou de teste leva o teto de dez minutos do
/// terminal, e o que pode passar disso roda por pacote, um por comando: a
/// frase mora na mesma linha que proíbe o segundo plano, nos dois moldes que
/// compilam — onda e revisor —, nos dois idiomas, e o teto vago do comando
/// saiu.
#[test]
fn templates_say_the_ten_minute_cap_on_build_and_test() {
    for (lang, rule, said, vague) in [
        (
            "pt-BR",
            "Nunca mande compilação ou teste para segundo plano",
            ["`timeout: 600000`", "dez minutos", "um pacote por comando"],
            "teto de tempo do comando",
        ),
        (
            "en-US",
            "Never send a build or test to the background",
            ["`timeout: 600000`", "ten minutes", "one package per command"],
            "command's time limit",
        ),
    ] {
        for name in ["wave", "review"] {
            let body = template(lang, name);
            let line = body.lines().find(|l| l.contains(rule)).unwrap_or_else(|| panic!("the {lang} `{name}` agent lost `{rule}`"));
            for phrase in said {
                assert!(line.contains(phrase), "the {lang} `{name}` agent does not say `{phrase}`: {line}");
            }
            assert!(!body.contains(vague), "the {lang} `{name}` agent still says `{vague}`");
        }
    }
}

/// Comentário e nome de teste descrevem o comportamento em palavras e nunca
/// citam código de item, número de onda, nome de spec, pendência ou o
/// Mustard: o molde de onda manda isso na linha dos comentários, e o
/// revisor confere nas linhas novas da obra, nos dois idiomas.
#[test]
fn templates_forbid_spec_codes_in_comments() {
    for (lang, comments, finding, review_check, cited) in [
        (
            "pt-BR",
            "- Comentários",
            "é achado",
            "## Como conferir",
            ["nome de teste", "código de item", "onda", "spec", "pendência", "Mustard"],
        ),
        (
            "en-US",
            "- Comments",
            "is a finding",
            "## How to check",
            ["test name", "item code", "wave", "spec", "pending item", "Mustard"],
        ),
    ] {
        let name = "wave";
        let body = template(lang, name);
        let line = body.lines().find(|l| l.starts_with(comments)).unwrap_or_else(|| panic!("the {lang} `{name}` agent has no comments line"));
        for word in cited {
            assert!(line.contains(word), "the {lang} `{name}` comments line does not name `{word}`: {line}");
        }
        let review = template(lang, "review");
        let check = section(&review, review_check);
        let line = check.lines().find(|l| l.contains(finding)).unwrap_or_else(|| panic!("the {lang} reviewer does not check comments:{check}"));
        for word in cited {
            assert!(line.contains(word), "the {lang} reviewer's comment check does not name `{word}`: {line}");
        }
    }
}

/// O revisor diz os dois caminhos da volta: na revisão final da obra, grava o
/// veredito pela ferramenta, com a linha de exemplo que a gravação aceita; na
/// revisão de um levantamento e no pull request de um colega, devolve o texto
/// a quem despachou. O pedido de cada uso diz qual vale: o do fechamento manda
/// gravar, o do levantamento e o da porta do pull request dizem que o texto
/// volta sem gravar nada.
#[test]
fn reviewer_template_states_the_two_paths() {
    for (lang, locale, header, said, survey_said) in [
        (
            "pt-BR",
            Locale::PtBr,
            "## O que devolver",
            ["revisão final da obra", "`run write verdict", "levantamento", "pull request", "devolva o texto a quem despachou"],
            "devolve o texto a você, sem gravar veredito",
        ),
        (
            "en-US",
            Locale::EnUs,
            "## What to return",
            ["final review", "`run write verdict", "survey", "pull request", "return the text to whoever dispatched you"],
            "returns its text to you, recording no verdict",
        ),
    ] {
        let review = template(lang, "review");
        let back = section(&review, header);
        for phrase in said {
            assert!(back.contains(phrase), "the {lang} reviewer does not say `{phrase}`:{back}");
        }
        assert!(!review.contains("<VERDICT>"), "the {lang} reviewer still teaches the pasted line");
        let example = back.lines().find(|l| l.starts_with("{\"final\"")).unwrap_or_else(|| panic!("no example line:{back}"));
        let parsed: Value = serde_json::from_str(example).unwrap_or_else(|e| panic!("{lang}: {e}: {example}"));
        assert_eq!(parsed["final"], json!(true), "{example}");

        let close = translate("close.final_review", locale);
        assert!(close.contains("run write verdict") && !close.contains("<VERDICT>"), "{close}");
        let survey = translate("survey.outside_review_step", locale);
        assert!(survey.contains(survey_said), "{survey}");
    }
    let pr = std::fs::read_to_string(repo_root().join("plugin/commands/pr.md")).unwrap();
    assert!(pr.contains("`mustard-review` agent, from that brief: it returns its text to you and records nothing"), "{pr}");
}

/// O erro que pode se repetir não vira lição no molde do revisor, nos dois
/// idiomas: o banco de lições fica só na máquina de quem programa e não vai
/// ao git. O revisor escreve como achado do veredito o conserto no código,
/// com o teste que falha se o erro voltar. Nenhuma frase do molde fala em
/// lição fora da linha de exemplo do veredito, que segue dizendo se uma
/// lição do pedido se repetiu; a proposta de mudança na skill continua.
#[test]
fn reviewer_proposes_the_fix_with_a_test_in_place_of_the_lesson() {
    for (lang, header, lesson_word, asked, skill) in [
        (
            "pt-BR",
            "## Propostas",
            "liç",
            ["Erro que pode se repetir?", "achado do veredito", "conserto no código", "teste que falha se o erro voltar"],
            "Proponha a mudança nela",
        ),
        (
            "en-US",
            "## Proposals",
            "lesson",
            ["A mistake that can happen again?", "finding of the verdict", "fix in the code", "test that fails if the mistake comes back"],
            "Propose the change to it",
        ),
    ] {
        let review = template(lang, "review");
        for line in review.lines().filter(|line| !line.starts_with('{')) {
            assert!(
                !line.to_lowercase().contains(lesson_word),
                "the {lang} reviewer still talks about lessons outside the verdict example: {line}"
            );
        }
        let proposals = section(&review, header);
        let line = proposals
            .lines()
            .find(|line| line.contains(asked[0]))
            .unwrap_or_else(|| panic!("the {lang} reviewer no longer says what to do with a mistake that repeats:{proposals}"));
        for phrase in asked {
            assert!(line.contains(phrase), "the {lang} reviewer does not say `{phrase}`: {line}");
        }
        assert!(proposals.contains(skill), "the {lang} reviewer lost the proposal to change a skill:{proposals}");
        let example = review.lines().find(|line| line.starts_with("{\"final\"")).unwrap_or_else(|| panic!("no {lang} example"));
        let parsed: Value = serde_json::from_str(example).unwrap_or_else(|e| panic!("{lang}: {e}: {example}"));
        assert!(parsed["lessons"].is_array(), "the verdict still says whether a lesson of the request repeated: {example}");
    }
}

/// O molde da onda e o do revisor, nos dois idiomas, mandam procurar código
/// pela porta, com o padrão original e a intenção em campos distintos.
/// O scan enriquece a pesquisa executada, sem apagar suas ocorrências.
#[test]
fn the_wave_and_review_agents_preserve_literal_searches_and_offer_explicit_map_recovery() {
    for (lang, literal, command, uncertain) in [
        ("pt-BR", "preserve as opções originais, sem Jev por busca literal.",
         "mustard-rt run search --shell-output", "não comprova cobertura nem ausência de uso"),
        ("en-US", "preserve the original options, without Jev for literal searches.",
         "mustard-rt run search --shell-output", "does not prove coverage or absence of use"),
    ] {
        for name in ["wave", "review"] {
            let agent = template(lang, name);
            assert!(agent.contains(literal), "{lang} {name}: original tool behavior must stay explicit");
            assert!(agent.contains(command), "{lang} {name}: routine searches use agent output instead of diagnostics");
            assert!(agent.contains(uncertain), "{lang} {name}: graph uncertainty must stay explicit");
            for obsolete in ["answers in place of the search", "responde no lugar da busca"] {
                assert!(!agent.contains(obsolete), "{lang} {name}: obsolete search instruction {obsolete}");
            }
            for line in agent.lines().filter(|line|line.contains("--query") || line.contains("--intent")) {
                assert!(line.contains("run search"),"task context belongs to the gateway, separate from the literal pattern: {line}");
            }
            assert!(agent.contains("run search") && agent.contains("--purpose"),"{lang} {name}: task-oriented evidence must be available");
        }
    }
}

/// As regras de execução que valem em qualquer projeto — ler por trecho, não
/// reler depois de editar, nada em segundo plano, não comitar nem usar `git
/// add`, rodar cada comando de dentro da cópia — moram só no molde do agente,
/// escritas à mão e fora do catálogo de textos, e o molde da onda e do revisor
/// levam as mesmas palavras, nos dois idiomas. Nenhum dos dois fala mais de
/// pasta de compilação: a cópia é a vaga fixa, com a compilação dentro. Rodar
/// só os testes do que mudou é da onda, que deixa a suíte inteira e o lint
/// para a rodada e não roda mais a suíte no fim; o revisor roda os testes que
/// lê e os que seus cortes derrubam, a suíte inteira uma vez no fim pelo
/// `rtk` e, na revisão final, não repete a suíte que o fechamento rodou do
/// `testCommand`.
#[test]
fn the_wave_and_review_agents_carry_the_project_wide_execution_rules() {
    for (lang, phrases, gone, wave_phase, final_phase) in [
        ("pt-BR", ["Use os comandos do mapa quando a localização ou evidência atual faltar",
          "Leia a faixa pertinente e expanda se faltar contexto", "Releia quando o conteúdo mudou ou a prova exigir",
          "Nunca mande compilação ou teste para segundo plano", "Não comite e não use `git add`: o commit é da rodada",
          "Rode cada comando de dentro da cópia", "o corte que mexe no mesmo trecho de outro vai sozinho"],
          ["pasta de compilação", "passa de uma cópia para a seguinte"],
          "A rodada executa o build e as provas pertinentes antes do commit", "A suíte inteira e o lint ficam na validação final da spec"),
        ("en-US", ["Use map commands when location or current evidence is missing",
          "Read the relevant range and expand when context is missing", "Reread when content changed or a proof requires it",
          "Never send a build or test to the background", "Do not commit and do not use `git add`: the commit belongs to the round",
          "Run every command from inside the copy", "a cut that touches the same spot as another goes alone"],
          ["build folder", "passes from one copy to the next"],
          "The round runs the build and pertinent criterion proofs before the commit", "The full suite and lint run during final spec validation"),
    ] {
        for name in ["wave", "review"] {
            let agent = template(lang, name);
            for phrase in phrases { assert!(agent.contains(phrase), "{lang} {name}: {phrase}"); }
            for phrase in gone { assert!(!agent.contains(phrase), "{lang} {name}: obsolete {phrase}"); }
        }
        let wave = template(lang, "wave");
        assert!(wave.contains(wave_phase) && wave.contains(final_phase), "{lang}: validation phases");
        let review = template(lang, "review");
        assert!(review.contains("`testCommand`") && review.contains("lint") && review.contains("`rtk`"));
        assert!(review.contains(if lang=="pt-BR" { "conteúdo, comando ou execução ficarem incertos" } else { "content, command or execution is uncertain" }));
        assert!(!review.contains("the close already ran it") && !review.contains("o fechamento já a rodou"));
    }
}

/// Nenhum texto de agente manda criar cópia do projeto por conta própria —
/// nem os dois que o projeto recebe, em cada idioma, nem as instruções fixas
/// que o binário monta no pedido da onda e da revisão —; a ordem de nunca
/// criar outra mora no pedido, que traz a cópia de cada um. O pedido
/// que a rodada monta,
/// pelo binário, mesmo num projeto que o mapa marca como Rust, traz a vaga
/// que ela preparou — cada onda na sua — e nenhuma pasta de compilação: o que
/// a cópia compila fica dentro dela. O aviso das sobras no disco continua no
/// catálogo do início da sessão, com o comando que as limpa.
#[test]
fn no_agent_text_creates_a_copy_on_its_own_and_the_request_names_the_slot_without_a_build_folder() {
    for (lang, text) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        let mut texts: Vec<(String, String)> =
            ["wave", "review"].iter().map(|name| (format!("{lang} {name}"), template(lang, name))).collect();
        for key in FIXED_PARTS {
            texts.push((format!("{lang} {key}"), translate(key, text).to_string()));
        }
        for (what, body) in &texts {
            for forbidden in COPY_ON_ITS_OWN {
                assert!(!body.contains(forbidden), "{what} still says `{forbidden}`");
            }
        }
        let said = if text == Locale::PtBr { "nunca crie outra" } else { "never create another" };
        for key in ["prompt.execution.copy", "prompt.review.copy"] {
            assert!(translate(key, text).contains(said), "the {lang} `{key}` does not say `{said}`");
        }
        // O revisor prova de ponta a ponta, pelo caminho que o usuário usa,
        // além dos testes. O molde vai a todo projeto: instalar o Mustard é a
        // prova deste repositório, e mora no CLAUDE.md da raiz dele.
        let (end_to_end, user_path) = if text == Locale::PtBr {
            ("prove de ponta a ponta", "pelo caminho que ele usa (o comando, a tela, a chamada)")
        } else {
            ("prove it end to end", "on the path they take (the command, the screen, the call)")
        };
        for line in [end_to_end, user_path, "mktemp -d"] {
            assert!(template(lang, "review").contains(line), "the {lang} reviewer does not say `{line}`");
        }
        assert!(!template(lang, "review").contains("mustard init"), "the {lang} reviewer carries this repository's own proof");

        let notice = translate("scratch.residue.notice", text);
        assert!(notice.contains("mustard-rt run clean"), "the {lang} disk notice lost its cleanup command");
    }

    let dir = tempfile::tempdir().unwrap();
    let (root, home) = installed(dir.path(), r#"{"version":"1.0.0","language":{"text":"pt-BR"},"maxCompilingWaves":2}"#);
    let opened = rt(&root, &home, &["run", "open", "--kind", "feature", "--name", "copia", "--base", "dev"], None);
    assert_eq!(opened["ok"], json!(true), "{opened}");
    let file = root.join(".claude/spec/copia/spec.ndjson");
    let put = |event_type: &str, body: Value| store::write(&file, event_type, body.as_object().cloned().unwrap(), &[]).unwrap().id;
    let said = put("message", json!({"author": "user", "text": "o objetivo"}));
    let crit = put("criterion", json!({"when": "a onda roda", "then": "passa", "proof": "true", "form": "ubiquitous", "origin": said}));
    // Cada onda declara o arquivo dela: a trava por arquivo tira da rodada
    // as ondas que dividem um mesmo arquivo, e este teste prova as duas
    // saindo juntas, sem cruzar arquivo nenhuma com a outra.
    for n in [1, 2] {
        put("wave", json!({"author": "binary", "n": n, "text": format!("Onda {n}."), "criteria": [crit], "done_when": "passa", "origin": said}));
        put("task", json!({"wave": n, "text": "Mexer no arquivo dela.", "files": [{"path": format!("src/onda{n}.rs")}], "origin": said}));
    }
    put("state", json!({"phase": "running", "branch": "feature/copia"}));
    // O mapa marca o projeto como Rust, como o scan o grava.
    let model = json!({"projects": [{"name": "(root)", "dir": "", "kind": "cargo", "code_files": 1}]});
    mustard_core::io::project_map::write_text(&root, &model.to_string()).unwrap();

    let round = rt(&root, &home, &["run", "round", "--spec", "copia"], None);
    assert_eq!(round["ok"], json!(true), "{round}");
    let dispatched = round["dispatch"].as_array().cloned().unwrap_or_default();
    assert_eq!(dispatched.len(), 2, "the two waves, each on its own file, go out together: {round}");
    let log = store::read(&file).unwrap().unwrap();
    let mut copies = Vec::new();
    for sent in dispatched {
        let wave = sent["wave"].as_u64().unwrap();
        let prompt = &request_by_command(&root, &home, &sent);
        let send = log.visible().into_iter().rfind(|e| e.event_type == "send" && e.wave() == Some(wave)).unwrap();
        let copy = send.str_field("copy").unwrap_or_else(|| panic!("wave {wave} recorded no copy: {round}"));
        assert!(send.str_field("build_dir").is_none(), "wave {wave} recorded a build folder: {round}");
        let slot = usize::try_from(wave).unwrap() - 1;
        let expected = copies_of(&home, &root).join("copia").join(mustard_core::io::wave_prompt::slot_name(slot));
        assert_eq!(copy, mustard_core::io::wave_prompt::shown(&expected), "the slot lives in the project's copies folder");
        assert!(!Path::new(copy).starts_with(&root), "the copy lives outside the project: {copy}");
        assert!(Path::new(copy).join(".git").is_file(), "the copy of wave {wave} is a linked checkout");
        assert!(prompt.contains(&format!("`{copy}`")), "the request names the copy: {prompt}");
        for word in ["CARGO_TARGET_DIR", "target/copias"] {
            assert!(!prompt.contains(word), "the request names no build folder ({word}): {prompt}");
        }
        assert!(prompt.contains(translate("prompt.return.loose", Locale::PtBr)), "{prompt}");
        assert!(!prompt.contains("nasce vermelho"), "the red proof lives in the agent text: {prompt}");
        copies.push(copy.to_string());
    }
    assert_ne!(copies[0], copies[1], "each wave works in its own slot");
}

/// O pedido de onda, montado pelo binário de verdade, diz com todas as
/// letras que o agente não comita — o trabalho fica mudado na cópia, sem
/// `git add` nem `git commit`, e quem junta e comita é a rodada — e que, no
/// relatório de entrega, o campo `commit` é o título da mensagem, nunca o
/// código do commit. As duas frases saem nos dois idiomas.
#[test]
fn the_wave_request_says_the_agent_never_commits_and_the_commit_field_is_the_title() {
    for (lang, text) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        let dir = tempfile::tempdir().unwrap();
        let (root, home) =
            installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));
        let opened = rt(&root, &home, &["run", "open", "--kind", "feature", "--name", "titulo", "--base", "dev"], None);
        assert_eq!(opened["ok"], json!(true), "{opened}");
        let file = root.join(".claude/spec/titulo/spec.ndjson");
        let put =
            |event_type: &str, body: Value| store::write(&file, event_type, body.as_object().cloned().unwrap(), &[]).unwrap().id;
        let said = put("message", json!({"author": "user", "text": "o objetivo"}));
        let crit = put("criterion", json!({"when": "a onda roda", "then": "passa", "proof": "true", "form": "ubiquitous", "origin": said}));
        put("wave", json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "passa", "origin": said}));
        put("task", json!({"wave": 1, "text": "Mexer no arquivo dela.", "files": [{"path": "src/onda1.rs"}], "origin": said}));
        put("state", json!({"phase": "running", "branch": "feature/titulo"}));

        let round = rt(&root, &home, &["run", "round", "--spec", "titulo"], None);
        assert_eq!(round["ok"], json!(true), "{round}");
        let dispatched = round["dispatch"].as_array().cloned().unwrap_or_default();
        assert_eq!(dispatched.len(), 1, "{round}");
        let prompt = &request_by_command(&root, &home, &dispatched[0]);
        for key in ["prompt.execution.no_commit", "prompt.execution.commit_field"] {
            let sentence = translate(key, text);
            assert!(prompt.contains(sentence), "{lang} wave request misses `{key}`: {prompt}");
        }
    }
}

/// O pedido de onda, montado pelo binário de verdade, manda o agente gravar
/// a entrega pela ferramenta, `run write delivered`, com todo o detalhe do
/// trabalho no campo de texto dela: a parte "O que devolver" do pedido diz
/// isso, junto do campo `commit` e da proibição de texto solto.
/// Nenhum texto ensina mais a linha colada na última mensagem: nem o pedido
/// da onda, nem o molde de onda — que traz a linha de exemplo, sem
/// marca, e o campo das sobras —, nem o pedido da revisão final. A instrução
/// volta ao orquestrador na saída da própria rodada: quando a volta de um
/// agente não estiver na spec, o `next` manda o agente gravá-la de novo. Nos
/// dois idiomas.
#[test]
fn request_says_to_write_the_delivery_through_the_tool() {
    for (lang, text) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        let dir = tempfile::tempdir().unwrap();
        let (root, home) =
            installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));
        let opened = rt(&root, &home, &["run", "open", "--kind", "feature", "--name", "gravada", "--base", "dev"], None);
        assert_eq!(opened["ok"], json!(true), "{opened}");
        let file = root.join(".claude/spec/gravada/spec.ndjson");
        let put =
            |event_type: &str, body: Value| store::write(&file, event_type, body.as_object().cloned().unwrap(), &[]).unwrap().id;
        let said = put("message", json!({"author": "user", "text": "o objetivo"}));
        let crit = put("criterion", json!({"when": "a onda roda", "then": "passa", "proof": "true", "form": "ubiquitous", "origin": said}));
        put("wave", json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "passa", "origin": said}));
        put("task", json!({"wave": 1, "text": "Mexer no arquivo dela.", "files": [{"path": "src/onda1.rs"}], "origin": said}));
        put("state", json!({"phase": "running", "branch": "feature/gravada"}));

        let round = rt(&root, &home, &["run", "round", "--spec", "gravada"], None);
        assert_eq!(round["ok"], json!(true), "{round}");
        let dispatched = round["dispatch"].as_array().cloned().unwrap_or_default();
        assert_eq!(dispatched.len(), 1, "{round}");
        let prompt = &request_by_command(&root, &home, &dispatched[0]);

        let commit_field = translate("prompt.execution.commit_field", text);
        let report_lines = translate("prompt.execution.report_lines", text);
        assert!(report_lines.contains("`mustard-rt run write delivered`"), "{lang}: {report_lines}");
        let returns = section(prompt, &format!("## {}", translate("prompt.part.return", text)));
        assert!(returns.contains(report_lines), "{lang} the return part misses the recording reminder: {prompt}");
        assert!(returns.contains(commit_field), "{lang} the return part misses `commit_field`: {prompt}");
        assert!(returns.contains(translate("prompt.return.loose", text)), "{lang} the return part misses the loose-text ban: {prompt}");
        assert!(!prompt.contains("<DELIVERED>"), "{lang} wave request still teaches the pasted line: {prompt}");

        let final_fixed = translate("prompt.final.fixed", text);
        assert!(final_fixed.contains("run write verdict") && !final_fixed.contains("<VERDICT>"), "{final_fixed}");

        let name = "wave";
        let body = std::fs::read_to_string(root.join(format!(".claude/agents/mustard/{name}.md"))).unwrap();
        let output = section(&body, wave_headers(lang).1);
        assert!(output.contains("`run write delivered --json"), "{lang} `{name}`:{output}");
        assert!(body.contains("\"leftovers\""), "{lang} `{name}` lacks the leftovers field");
        assert!(!body.contains("<DELIVERED>"), "{lang} `{name}` still teaches the pasted line");
        let example = output.lines().find(|l| l.starts_with("{\"wave\"")).unwrap_or_else(|| panic!("no example line:{output}"));
        let parsed: Value = serde_json::from_str(example).unwrap_or_else(|e| panic!("{lang} `{name}`: {e}: {example}"));
        assert_eq!(parsed["wave"], json!(1), "{example}");

        // A segunda metade: a instrução que a rodada devolve ao orquestrador
        // depois de despachar, mandando o agente gravar de novo quando a volta
        // dele não estiver na spec, e pedindo só a linha de consumo.
        let next = round["next"].as_str().unwrap_or_default();
        let record_again = match text {
            Locale::PtBr => "mande o agente gravá-la de novo pela ferramenta",
            Locale::EnUs => "have the agent record it again through the tool",
        };
        assert!(next.contains(record_again), "{lang} round output misses the instruction to record again: {next}");
        assert!(next.contains("<USAGE>"), "{lang} round output does not ask for the usage line: {next}");
        assert!(!next.contains("<DELIVERED>") && !next.contains("<VERDICT>"), "{lang} round output teaches a pasted line: {next}");
    }
}

/// A parte fixa de cada pedido que o binário monta: o da onda e o da revisão
/// final do conjunto, com o que olhar da primeira revisão e o da revisão de
/// volta, e a linha dos idiomas do projeto, que abre todo pedido a um agente.
const FIXED_PARTS: [&str; 5] =
    ["prompt.fixed", "prompt.final.fixed", "prompt.final.look", "prompt.final.look_again", "prompt.languages"];

/// Quantas palavras seguidas fazem uma frase repetida.
const REPEATED_RUN: usize = 6;

/// As palavras de um trecho, em minúsculas, sem pontuação nem marcação.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// Uma instrução mora num lugar só: nenhuma frase dos textos do agente de
/// onda e do revisor, em cada idioma, se repete na parte fixa de um pedido —
/// nem seis palavras seguidas de uma frase delas. A parte fixa fica com o que
/// o pedido é e o que devolver, e cada texto de agente segue no teto.
#[test]
fn the_fixed_part_of_a_request_repeats_no_sentence_of_the_agent_texts() {
    for (lang, text) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        let fixed: Vec<(&str, String)> =
            FIXED_PARTS.iter().map(|key| (*key, format!(" {} ", words(translate(key, text)).join(" ")))).collect();
        for name in ["wave", "review"] {
            let agent = template(lang, name);
            for sentence in agent.split(['.', ':', ';', '?', '!', '\n']) {
                for run in words(sentence).windows(REPEATED_RUN) {
                    let needle = format!(" {} ", run.join(" "));
                    for (key, body) in &fixed {
                        assert!(!body.contains(&needle), "{lang} {key} repeats `{}` from the `{name}` agent", needle.trim());
                    }
                }
            }
        }
    }
}

/// Roda, pelo binário de verdade, a linha inteira que a resposta `report`
/// devolveu em `command`. Uma linha que o parser recusa sai com código 2 e sem
/// resposta JSON, e o [`rt`] cai dizendo o erro do parser.
fn run_returned(root: &Path, home: &Path, report: &Value) -> Value {
    let line = report["command"].as_str().unwrap_or_else(|| panic!("the answer returns no command: {report}"));
    let argv: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(argv.first(), Some(&"mustard-rt"), "{line}");
    rt(root, home, &argv[1..], None)
}

/// Cada comando do fluxo responde o próximo passo, e o comando que ele
/// devolve roda inteiro, sem o parser recusar opção nenhuma: a retomada no
/// levantamento devolve o levantamento; a rodada sem nada a despachar e com
/// tudo entregue devolve o fechamento; o fechamento, mesmo sem onda nenhuma,
/// pede o agente de teste dedicado, e aprovado ele devolve o pull request com
/// a base e a branch da spec; e a retomada da spec fechada devolve a mesma
/// linha. O que o modelo roda a seguir vem dessa resposta, nunca de um texto
/// do Mustard.
#[test]
fn every_flow_command_answers_its_next_step() {
    let dir = tempfile::tempdir().unwrap();
    // Um provedor que ninguém atende: o pull request roda até o provedor e
    // responde, sem sair da máquina.
    let (root, home) = installed(
        dir.path(),
        r#"{"version":"1.0.0","language":{"text":"pt-BR"},"git":{"flow":{"*":"dev","dev":"main"},"provider":"nenhum"}}"#,
    );
    let said = |report: &Value, field: &str| report[field].as_str().is_some_and(|t| !t.trim().is_empty());

    let opened = rt(&root, &home, &["run", "open", "--kind", "feature", "--name", "passo", "--base", "dev"], None);
    assert_eq!(opened["ok"], json!(true), "{opened}");
    assert!(said(&opened, "hint") && said(&opened, "question"), "the opening says no next step: {opened}");

    let surveyed = rt(&root, &home, &["run", "grill", "--spec", "passo", "--kinds", "feature"], None);
    assert!(said(&surveyed, "hint"), "the survey says no next step: {surveyed}");

    let resumed = rt(&root, &home, &["run", "resume", "--spec", "passo"], None);
    assert!(said(&resumed, "next"), "the resume says no next step: {resumed}");
    assert_eq!(resumed["command"], json!("mustard-rt run grill --spec passo"), "{resumed}");
    // A linha chega ao levantamento, que responde por si: aqui, que falta o
    // objetivo.
    let again = run_returned(&root, &home, &resumed);
    assert_eq!(again["reason"], json!("goal-missing"), "{again}");

    // A spec em execução, gravada direto no arquivo de eventos, sem onda
    // nenhuma: nada a despachar, nada a revisar, e nada que falte.
    let file = root.join(".claude/spec/passo/spec.ndjson");
    let running = json!({"phase": "running", "branch": "feature/passo"});
    store::write(&file, "state", running.as_object().cloned().unwrap(), &[]).unwrap();
    let round = rt(&root, &home, &["run", "round", "--spec", "passo"], None);
    assert_eq!(round["command"], json!("mustard-rt run close --spec passo"), "{round}");
    let close = translate("round.close", Locale::PtBr).replace("{command}", "mustard-rt run close --spec passo");
    assert!(round["next"].as_str().is_some_and(|next| next.ends_with(&close)), "{round}");

    // Mesmo sem onda nenhuma, o fechamento pede o agente de teste dedicado.
    let asked = run_returned(&root, &home, &round);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    let approved = json!({"final": true, "result": "approved", "text": "Está pronto."});
    let written = rt(&root, &home, &["run", "write", "verdict", "--spec", "passo", "--json", &approved.to_string()], None);
    assert_eq!(written["ok"], json!(true), "{written}");
    let closed = rt(&root, &home, &["run", "close", "--spec", "passo"], None);
    assert_eq!(closed["ok"], json!(true), "{closed}");
    let pr_open = "mustard-rt run pr-open --base dev --head feature/passo --spec passo";
    assert_eq!(closed["command"], json!(pr_open), "{closed}");
    let then = translate("close.next", Locale::PtBr).replace("{command}", pr_open);
    assert!(closed["next"].as_str().is_some_and(|next| next.ends_with(&then)), "{closed}");
    let asked = run_returned(&root, &home, &closed);
    assert_eq!(asked["provider"], json!("nenhum"), "the pull request line ran up to the provider: {asked}");

    let resumed = rt(&root, &home, &["run", "resume", "--spec", "passo"], None);
    assert!(said(&resumed, "next"), "the resume says no next step: {resumed}");
    assert_eq!(resumed["command"], json!(pr_open), "{resumed}");
    let asked = run_returned(&root, &home, &resumed);
    assert_eq!(asked["provider"], json!("nenhum"), "{asked}");
}
