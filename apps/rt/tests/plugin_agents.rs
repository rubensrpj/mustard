// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Os textos de agente do Mustard, pelo binário de verdade.
//!
//! O projeto recebe exatamente quatro agentes — `mustard-wave`,
//! `mustard-wave-solo`, `mustard-review` e `mustard-skill` —, no idioma do
//! `language.text`; os dois idiomas existem como molde do produto; nenhum
//! texto manda criar cópia do projeto por conta própria, e os de onda e de
//! revisão mandam trabalhar na cópia e na pasta de compilação que o pedido
//! indica; e cada comando do fluxo responde o próximo passo, que o modelo não
//! escolhe sozinho. O que prende o texto de um agente é o que ele diz, não
//! quantos bytes ele tem. Nenhum dos dois agentes de onda traz teto de idas e
//! voltas no cabeçalho: os tetos de dez e quinze cortavam toda onda real no
//! meio, e a onda cortada recomeçava do zero.

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

/// O projeto recebe exatamente os quatro agentes, no idioma do
/// `language.text`; os dois idiomas existem como molde; e o
/// plugin não entrega agente nenhum, porque entregaria os dois idiomas. Os
/// textos que o instalador escreve trazem as duas guardas desta obra: provar
/// que nada se perde antes de apagar ou mover alguma coisa no git, e o teste
/// do caso em que o "antes" falha quando o critério diz "só depois de".
#[test]
fn the_project_receives_exactly_four_agents_in_its_text_language() {
    for (lang, other) in [("pt-BR", "en-US"), ("en-US", "pt-BR")] {
        let dir = tempfile::tempdir().unwrap();
        let (root, _home) = installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));

        let agents = files_under(&root.join(".claude/agents"));
        assert_eq!(
            agents,
            ["mustard/review.md", "mustard/skill.md", "mustard/wave-solo.md", "mustard/wave.md"],
            "the {lang} project got another set of agent texts",
        );
        for name in ["wave", "review", "skill", "wave-solo"] {
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

/// O instalador escreve os dois moldes de agente de onda — o de lote de uma
/// tarefa (`wave-solo.md`) e o de lote de várias (`wave.md`) —, e nenhum dos
/// dois traz teto de idas e voltas no cabeçalho. A medição das vinte e cinco
/// ondas entregues desta obra deu gasto de 36 a 403 idas, com média de 153:
/// nenhuma onda real cabia nos tetos de dez e quinze que havia aqui, e a onda
/// cortada recomeçava do zero. Quem cuida da janela cheia é a compactação e
/// quem cuida da onda parada é o sinal de vida da rodada. Nenhum dos dois pede
/// mais um relatório pelo tamanho: a entrega vai gravada na spec.
#[test]
fn o_molde_do_agente_de_onda_nao_traz_teto_de_idas_e_voltas() {
    for (lang, tokens) in [("pt-BR", "mil e dois mil tokens"), ("en-US", "one and two thousand tokens")] {
        let dir = tempfile::tempdir().unwrap();
        let (root, _home) = installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));

        for name in ["wave", "wave-solo"] {
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
}

/// O nome de cada agente do Mustard leva o prefixo do Mustard, e um projeto
/// que já tem um agente chamado `review` fica com os dois: o dele, intocado,
/// e o `mustard-review`. Uma instalação antiga, com os nomes sem prefixo, é
/// migrada pela instalação seguinte, e a rodada manda cada pedido ao agente
/// pelo nome com prefixo.
#[test]
fn the_mustard_agents_carry_the_prefix_and_live_beside_a_project_agent_of_the_same_name() {
    let dir = tempfile::tempdir().unwrap();
    let (root, home) = installed(dir.path(), r#"{"version":"1.0.0","language":{"text":"pt-BR"}}"#);
    let own = "---\nname: review\ndescription: O revisor do próprio projeto.\n---\n\nRevise.\n";
    std::fs::write(root.join(".claude/agents/review.md"), own).unwrap();
    // A instalação antiga: os três agentes do Mustard sem o prefixo.
    for name in ["wave", "review", "skill"] {
        let path = root.join(format!(".claude/agents/mustard/{name}.md"));
        let old = std::fs::read_to_string(&path).unwrap().replacen(&format!("name: mustard-{name}"), &format!("name: {name}"), 1);
        std::fs::write(&path, old).unwrap();
    }

    let report = rt(&root, &home, &["run", "upsert"], None);
    assert!(report.get("error").is_none(), "{report}");

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
        ["mustard-review", "mustard-skill", "mustard-wave", "mustard-wave-solo", "review"],
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
            .find(|l| l.contains("Nunca comite") || l.contains("Never commit"))
            .unwrap_or_else(|| panic!("the {lang} wave agent lost its git guard line"));
        assert!(guard.contains(phrase), "the {lang} wave agent does not forbid the stash: {guard}");
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

/// Os quatro moldes — onda de lote, onda de tarefa única, revisão e skill —
/// declaram o modelo opus com o esforço xhigh, nos dois idiomas: cada agente
/// sabe o próprio modelo e o próprio esforço, sem herdar o da sessão em
/// silêncio. Teto de idas e voltas não anda junto: nenhum dos dois moldes de
/// onda traz um, e quem prende isso é o teste do teto. Nenhum teste desta
/// obra recusa um molde pelo tamanho em bytes — o que prende o texto é o que
/// ele diz.
#[test]
fn each_agent_template_declares_its_own_model_and_effort() {
    for lang in ["pt-BR", "en-US"] {
        for name in ["wave", "wave-solo", "review", "skill"] {
            let text = template(lang, name);
            assert!(text.contains("\nmodel: opus\n"), "the {lang} `{name}` agent does not declare the opus model:\n{text}");
            assert!(text.contains("\neffort: xhigh\n"), "the {lang} `{name}` agent does not declare the xhigh effort:\n{text}");
            assert!(!text.contains("model: inherit"), "the {lang} `{name}` agent still inherits the session's model");
            assert!(!text.contains("model: sonnet"), "the {lang} `{name}` agent still asks for sonnet:\n{text}");
        }

        let wave = template(lang, "wave");
        assert!(wave.len() as u64 > 3_072, "the {lang} wave agent is not over the old byte cap, so it proves nothing");
    }
}

/// A skill que o agente de skill escreve nasce com o esforço no cabeçalho: o
/// molde dele, nos dois idiomas, manda pôr `effort: xhigh` ao lado de `name`
/// e `description`.
#[test]
fn the_skill_agent_asks_for_the_effort_in_the_skill_it_writes() {
    for lang in ["pt-BR", "en-US"] {
        let skill = template(lang, "skill");
        let header = skill
            .lines()
            .find(|line| line.contains("`name: <") && line.contains("`description:"))
            .unwrap_or_else(|| panic!("o molde {lang} não descreve o cabeçalho da skill gravada:\n{skill}"));
        assert!(
            header.contains("`effort: xhigh`"),
            "the {lang} skill agent does not ask for the effort in the skill it writes: {header}"
        );
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

/// Os títulos da fronteira da tarefa e do formato de saída, por idioma.
fn wave_headers(lang: &str) -> (&'static str, &'static str) {
    if lang == "pt-BR" { ("## Fronteira da tarefa", "## Formato de saída") } else { ("## Task boundary", "## Output format") }
}

/// Os dois moldes do agente de onda, nos dois idiomas, mandam mudar o arquivo
/// que a mesma mudança exige, mesmo fora da lista da tarefa, e dizê-lo em
/// `files`; a mudança de plano fica para o critério que precisa mudar ou a
/// spec que não diz o que fazer, e sai ao perceber, antes de explorar o
/// resto. A fronteira não manda mais parar porque falta arquivo na lista.
#[test]
fn o_molde_da_onda_poe_na_tarefa_o_arquivo_que_a_mudanca_exige() {
    for (lang, said, gone) in [
        ("pt-BR", ["entra no trabalho", "antes de explorar", "`files`", "`replan`"], "arquivo que falta"),
        ("en-US", ["is part of the work", "before exploring", "`files`", "`replan`"], "a missing file"),
    ] {
        for name in ["wave", "wave-solo"] {
            let body = template(lang, name);
            let boundary = section(&body, wave_headers(lang).0);
            for phrase in said {
                assert!(boundary.contains(phrase), "the {lang} `{name}` boundary does not say `{phrase}`:{boundary}");
            }
            assert!(!body.contains(gone), "the {lang} `{name}` agent still stops for `{gone}`:{boundary}");
        }
    }
}

/// A fronteira dos dois moldes de onda, nos dois idiomas, manda tirar na
/// mesma onda o que a própria mudança deixou sem uso, com o teste que só
/// existia para ele; num arquivo de outra onda em andamento, o agente não
/// edita e deixa a sobra em `leftovers`, o campo da entrega que a rodada vira
/// pendência.
#[test]
fn a_fronteira_manda_tirar_o_que_a_mudanca_deixou_sem_uso() {
    for (lang, said) in [
        ("pt-BR", ["deixa sem uso", "com o teste só dele", "sai na mesma onda", "outra onda em andamento, não edite"]),
        ("en-US", ["leaves unused", "with the test only it had", "goes in the same wave", "another running wave, do not edit"]),
    ] {
        for name in ["wave", "wave-solo"] {
            let body = template(lang, name);
            let boundary = section(&body, wave_headers(lang).0);
            for phrase in said.iter().chain(&["\"leftovers\":[{\"title\"", "\"detail\""]) {
                assert!(boundary.contains(phrase), "the {lang} `{name}` boundary does not say `{phrase}`:{boundary}");
            }
        }
    }
}

/// Todo comando de compilação ou de teste leva o teto de dez minutos do
/// terminal, e o que pode passar disso roda por pacote, um por comando: a
/// frase mora na mesma linha que proíbe o segundo plano, nos três moldes que
/// compilam — onda de lote, onda de tarefa única e revisor —, nos dois
/// idiomas, e o teto vago do comando saiu.
#[test]
fn os_moldes_mandam_o_teto_de_dez_minutos_em_compilacao_e_teste() {
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
        for name in ["wave", "wave-solo", "review"] {
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
/// Mustard: os dois moldes de onda mandam isso na linha dos comentários, e o
/// revisor confere nas linhas novas da obra, nos dois idiomas.
#[test]
fn os_moldes_proibem_codigo_da_spec_no_comentario() {
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
        for name in ["wave", "wave-solo"] {
            let body = template(lang, name);
            let line = body.lines().find(|l| l.starts_with(comments)).unwrap_or_else(|| panic!("the {lang} `{name}` agent has no comments line"));
            for word in cited {
                assert!(line.contains(word), "the {lang} `{name}` comments line does not name `{word}`: {line}");
            }
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
fn o_molde_do_revisor_diz_os_dois_caminhos() {
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

/// As regras de execução que valem em qualquer projeto — ler por trecho, não
/// reler depois de editar, rodar só os testes do que mudou, a suíte inteira
/// uma vez no fim pelo `rtk`, nada em segundo plano, não comitar nem usar
/// `git add`, rodar cada comando de dentro da cópia e a pasta de compilação
/// fixa — moram só no molde do agente, escritas à mão: o catálogo não guarda
/// mais essas frases (onda 11), e o molde da onda e do revisor levam as
/// mesmas palavras, nos dois idiomas.
#[test]
fn the_wave_and_review_agents_carry_the_project_wide_execution_rules() {
    let pt_br = [
        "Leia por trecho: ache a função com a busca e leia só ela",
        "Não releia o arquivo depois de editar: a edição já mostra o trecho mudado",
        "Durante o trabalho, rode só os testes do que mudou",
        "A suíte inteira roda uma vez no fim, em primeiro plano",
        "Nunca mande compilação ou teste para segundo plano",
        "Não comite e não use `git add`: o commit é da rodada",
        "Rode cada comando de dentro da cópia",
        "passa de uma cópia para a seguinte",
        "o corte que mexe no mesmo trecho de outro vai sozinho",
    ];
    let en_us = [
        "Read by excerpt: find the function with search and read only it",
        "Do not reread the file after editing: the edit already shows the changed excerpt",
        "During the work, run only the tests of what changed",
        "The whole suite runs once at the end, in the foreground",
        "Never send a build or test to the background",
        "Do not commit and do not use `git add`: the commit belongs to the round",
        "Run every command from inside the copy",
        "passes from one copy to the next",
        "a cut that touches the same spot as another goes alone",
    ];
    for (lang, phrases) in [("pt-BR", pt_br), ("en-US", en_us)] {
        for name in ["wave", "review"] {
            let agent = template(lang, name);
            for phrase in phrases {
                assert!(agent.contains(phrase), "the {lang} `{name}` agent lost the execution rule `{phrase}`");
            }
        }
    }
}

/// Nenhum texto de agente manda criar cópia do projeto por conta própria —
/// nem os três que o projeto recebe, em cada idioma, nem as instruções fixas
/// que o binário monta no pedido da onda e da revisão —; os de onda e de
/// revisão mandam trabalhar na cópia separada que o pedido indica e usar a
/// pasta de compilação quando ele indicar uma. O pedido que a rodada monta,
/// pelo binário, num projeto que o mapa marca como Rust, traz a cópia que ela
/// criou e a pasta de compilação. O aviso das sobras no disco continua no
/// catálogo do início da sessão, com o comando que as limpa.
#[test]
fn no_agent_text_creates_a_copy_on_its_own_and_the_request_names_the_copy_and_the_build_folder() {
    for (lang, text) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        let mut texts: Vec<(String, String)> =
            ["wave", "review", "skill"].iter().map(|name| (format!("{lang} {name}"), template(lang, name))).collect();
        for key in FIXED_PARTS {
            texts.push((format!("{lang} {key}"), translate(key, text).to_string()));
        }
        for (what, body) in &texts {
            for forbidden in COPY_ON_ITS_OWN {
                assert!(!body.contains(forbidden), "{what} still says `{forbidden}`");
            }
        }
        let said: [&str; 3] = if text == Locale::PtBr {
            ["cópia separada que o pedido indica", "se ele indicar uma pasta de compilação, use-a", "Nunca crie cópia por conta própria"]
        } else {
            ["separate copy the request names", "if it names a build folder, use it", "Never create a copy on your own"]
        };
        for name in ["wave", "review"] {
            for line in said {
                assert!(template(lang, name).contains(line), "the {lang} `{name}` agent does not say `{line}`");
            }
        }
        // O revisor prova de ponta a ponta, numa pasta temporária com o
        // Mustard instalado, além dos testes.
        let end_to_end = if text == Locale::PtBr { "prove de ponta a ponta" } else { "prove it end to end" };
        for line in [end_to_end, "mktemp -d", "`mustard init`"] {
            assert!(template(lang, "review").contains(line), "the {lang} reviewer does not say `{line}`");
        }

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
    std::fs::write(mustard_core::io::project_map::model_path(&root), model.to_string()).unwrap();

    let round = rt(&root, &home, &["run", "round", "--spec", "copia"], None);
    assert_eq!(round["ok"], json!(true), "{round}");
    let dispatched = round["dispatch"].as_array().cloned().unwrap_or_default();
    assert_eq!(dispatched.len(), 2, "the two waves, each on its own file, go out together: {round}");
    let log = store::read(&file).unwrap().unwrap();
    let mut dirs = Vec::new();
    for sent in dispatched {
        let wave = sent["wave"].as_u64().unwrap();
        let prompt = sent["prompt"].as_str().unwrap_or_default();
        let send = log.visible().into_iter().rfind(|e| e.event_type == "send" && e.wave() == Some(wave)).unwrap();
        let copy = send.str_field("copy").unwrap_or_else(|| panic!("wave {wave} recorded no copy: {round}"));
        let build = send.str_field("build_dir").unwrap_or_else(|| panic!("wave {wave} recorded no build folder"));
        assert!(copy.ends_with(&format!("/.claude/worktrees/mustard-copia-{wave}")), "{copy}");
        assert!(Path::new(copy).join(".git").is_file(), "the copy of wave {wave} is a linked checkout");
        assert!(build.contains("/target/copias/"), "{build}");
        assert!(prompt.contains(&format!("`{copy}`")), "the request names the copy: {prompt}");
        assert!(prompt.contains(&format!("`CARGO_TARGET_DIR={build}`")), "the request names the build folder: {prompt}");
        assert!(prompt.contains(translate("prompt.fixed", Locale::PtBr)), "{prompt}");
        assert!(!prompt.contains("nasce vermelho"), "the red proof lives in the agent text: {prompt}");
        dirs.push(build.to_string());
    }
    assert_ne!(dirs[0], dirs[1], "each copy builds in its own folder");
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
        let prompt = dispatched[0]["prompt"].as_str().unwrap_or_default();
        for key in ["prompt.execution.no_commit", "prompt.execution.commit_field"] {
            let sentence = translate(key, text);
            assert!(prompt.contains(sentence), "{lang} wave request misses `{key}`: {prompt}");
        }
    }
}

/// O pedido de onda, montado pelo binário de verdade, manda o agente gravar
/// a entrega pela ferramenta, `run write delivered`, com todo o detalhe do
/// trabalho no campo de texto dela: a parte fixa do pedido diz isso, e a
/// regra da execução repete logo depois das duas frases sobre não comitar.
/// Nenhum texto ensina mais a linha colada na última mensagem: nem o pedido
/// da onda, nem os dois moldes de onda — que trazem a linha de exemplo, sem
/// marca, e o campo das sobras —, nem o pedido da revisão final. A instrução
/// volta ao orquestrador na saída da própria rodada: quando a volta de um
/// agente não estiver na spec, o `next` manda o agente gravá-la de novo. Nos
/// dois idiomas.
#[test]
fn o_pedido_manda_gravar_a_entrega_pela_ferramenta() {
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
        let prompt = dispatched[0]["prompt"].as_str().unwrap_or_default();

        let fixed = translate("prompt.fixed", text);
        assert!(prompt.contains(fixed), "{lang} wave request misses the fixed part: {prompt}");
        assert!(fixed.contains("`mustard-rt run write delivered`"), "{lang} fixed part: {fixed}");

        let commit_field = translate("prompt.execution.commit_field", text);
        let report_lines = translate("prompt.execution.report_lines", text);
        assert!(report_lines.contains("`mustard-rt run write delivered`"), "{lang}: {report_lines}");
        assert!(prompt.contains(report_lines), "{lang} wave request misses the recording reminder: {prompt}");
        let commit_at = prompt.find(commit_field).unwrap_or_else(|| panic!("{lang} wave request misses `commit_field`: {prompt}"));
        let report_at = prompt.find(report_lines).unwrap();
        assert!(
            report_at > commit_at,
            "{lang} the recording reminder does not sit right after the no-commit phrases in the execution rules block: {prompt}"
        );
        assert!(!prompt.contains("<DELIVERED>"), "{lang} wave request still teaches the pasted line: {prompt}");

        let final_fixed = translate("prompt.final.fixed", text);
        assert!(final_fixed.contains("run write verdict") && !final_fixed.contains("<VERDICT>"), "{final_fixed}");

        for name in ["wave", "wave-solo"] {
            let body = std::fs::read_to_string(root.join(format!(".claude/agents/mustard/{name}.md"))).unwrap();
            let output = section(&body, wave_headers(lang).1);
            assert!(output.contains("`run write delivered --json"), "{lang} `{name}`:{output}");
            assert!(body.contains("\"leftovers\""), "{lang} `{name}` lacks the leftovers field");
            assert!(!body.contains("<DELIVERED>"), "{lang} `{name}` still teaches the pasted line");
            let example = output.lines().find(|l| l.starts_with("{\"wave\"")).unwrap_or_else(|| panic!("no example line:{output}"));
            let parsed: Value = serde_json::from_str(example).unwrap_or_else(|e| panic!("{lang} `{name}`: {e}: {example}"));
            assert_eq!(parsed["wave"], json!(1), "{example}");
        }

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
/// final do conjunto.
const FIXED_PARTS: [&str; 2] = ["prompt.fixed", "prompt.final.fixed"];

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
