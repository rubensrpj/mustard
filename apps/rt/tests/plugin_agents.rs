// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Os textos de agente do Mustard, pelo binário de verdade.
//!
//! O projeto recebe exatamente três agentes — `wave`, `review` e `skill` —, no
//! idioma do `language.text` e com até 3.072 bytes cada; os dois idiomas
//! existem como molde do produto; nenhum texto manda copiar o projeto nem
//! compilar numa cópia; e cada comando do fluxo responde o próximo passo, que
//! o modelo não escolhe sozinho.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

/// O teto de um texto de agente, em bytes.
const AGENT_CAP: usize = 3_072;

/// O jeito de mandar copiar o projeto ou compilar numa cópia: a cópia em si,
/// a pasta de compilação compartilhada que só servia às cópias, e a porta que
/// apagava a cópia depois.
const COPY_OR_BUILD_ELSEWHERE: &[&str] = &[
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

/// O projeto recebe exatamente os três agentes, no idioma do `language.text`
/// e com até 3.072 bytes cada; os dois idiomas existem como molde; e o
/// plugin não entrega agente nenhum, porque entregaria os dois idiomas.
#[test]
fn the_project_receives_exactly_three_agents_in_its_text_language() {
    for (lang, other) in [("pt-BR", "en-US"), ("en-US", "pt-BR")] {
        let dir = tempfile::tempdir().unwrap();
        let (root, _home) = installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));

        let agents = files_under(&root.join(".claude/agents"));
        assert_eq!(
            agents,
            ["mustard/review.md", "mustard/skill.md", "mustard/wave.md"],
            "the {lang} project got another set of agent texts",
        );
        for name in ["wave", "review", "skill"] {
            let installed = std::fs::read_to_string(root.join(format!(".claude/agents/mustard/{name}.md"))).unwrap();
            assert!(installed.len() <= AGENT_CAP, "the {lang} `{name}` agent is {} bytes", installed.len());
            assert_eq!(installed, template(lang, name), "the {lang} project got another text for `{name}`");
            assert_ne!(installed, template(other, name), "the {lang} and {other} `{name}` texts are the same");
            assert!(
                installed.starts_with(&format!("---\nname: {name}\n")),
                "the `{name}` file does not declare the agent `{name}`",
            );
        }
    }
    assert!(!repo_root().join("plugin/agents").exists(), "the plugin ships agent texts of its own");
}

/// Nenhum texto de agente manda copiar o projeto nem compilar numa cópia —
/// nem os três que o projeto recebe, em cada idioma, nem as instruções fixas
/// que o binário monta no pedido da onda e da revisão — e os dois que rodam
/// código dizem que não se copia. O aviso das sobras no disco continua no
/// catálogo do início da sessão, com o comando que as limpa.
#[test]
fn no_agent_text_tells_to_copy_the_project_or_build_in_a_copy() {
    for (lang, text) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        let mut texts: Vec<(String, String)> =
            ["wave", "review", "skill"].iter().map(|name| (format!("{lang} {name}"), template(lang, name))).collect();
        for key in ["prompt.fixed", "prompt.review.fixed"] {
            texts.push((format!("{lang} {key}"), translate(key, text).to_string()));
        }
        for (what, body) in &texts {
            for forbidden in COPY_OR_BUILD_ELSEWHERE {
                assert!(!body.contains(forbidden), "{what} still says `{forbidden}`");
            }
        }
        let never_copy = if text == Locale::PtBr { "Nunca copie" } else { "Never copy" };
        for name in ["wave", "review"] {
            assert!(template(lang, name).contains(never_copy), "the {lang} `{name}` agent never forbids the copy");
        }

        let notice = translate("scratch.residue.notice", text);
        assert!(notice.contains("mustard-rt run clean"), "the {lang} disk notice lost its cleanup command");
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
/// levantamento devolve o levantamento; a rodada sem nada a despachar, sem
/// revisão pendente e com tudo entregue e aprovado devolve o fechamento; o
/// fechamento devolve o pull request com a base e a branch da spec; e a
/// retomada da spec fechada devolve a mesma linha. O que o modelo roda a
/// seguir vem dessa resposta, nunca de um texto do Mustard.
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

    let closed = run_returned(&root, &home, &round);
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
