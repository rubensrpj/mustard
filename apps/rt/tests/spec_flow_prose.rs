// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! O próximo passo sai da resposta do comando, e a prosa só o repassa.
//!
//! A ordem dos passos não mora em texto nenhum: cada comando termina dizendo
//! qual é o próximo, e quem lê a resposta repassa o que ela traz. Enquanto
//! isso não era conferido, a prosa podia ensinar uma ordem que o binário não
//! segue — e foi assim que um texto continuou mandando rodar um comando que
//! já tinha saído.
//!
//! O que cada teste prende:
//!
//! 1. a tabela do próximo passo só nomeia comandos que a superfície publica;
//! 2. a resposta de verdade do `resume` traz a fase, o passo em palavras e o
//!    comando — os três campos, numa spec recém-aberta;
//! 3. a porta que o usuário tem manda repassar o campo `command` e proíbe
//!    escolher o passo por conta própria.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_rt::commands::flow::resume::NEXT_BY_PHASE;

/// A raiz do repositório, a partir deste crate (`apps/rt`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Os nomes que o `run --help` publica, lidos do mesmo retrato que a catraca
/// da superfície lê — nunca uma segunda lista.
fn published_names() -> Vec<String> {
    let path = repo_root().join("apps/rt/tests/fixtures/run-surface.txt");
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("o retrato da superfície não abriu: {e}"))
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Um repositório com `main` e `dev`, as bases declaradas, parado em `dev`.
fn project(root: &Path) {
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} falhou");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "t"]);
    git(&["checkout", "-q", "-b", "main"]);
    fs::write(root.join(".git").join("info").join("exclude"), ".claude/\nmustard.json\n").unwrap();
    fs::write(
        root.join("mustard.json"),
        r#"{"version":"1.0.0","language":{"text":"pt-BR"},"git":{"flow":{"*":"dev","dev":"main"}}}"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("main.rs"), "fn main() {}\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "init"]);
    git(&["checkout", "-q", "-b", "dev"]);
}

/// Roda um comando do binário na pasta `dir` e devolve a saída como JSON.
fn run(dir: &Path, args: &[&str]) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .arg("run")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("o binário roda");
    let texto = String::from_utf8_lossy(&out.stdout).to_string();
    serde_json::from_str(&texto).unwrap_or_else(|e| {
        panic!("a resposta de {args:?} não é JSON ({e}): {texto}{}", String::from_utf8_lossy(&out.stderr))
    })
}

/// Cada comando que a tabela do próximo passo nomeia é um comando publicado.
///
/// É esta tabela que dá chamador a cada passo do fluxo: nenhum texto diz a
/// ordem. Um nome errado aqui não quebra a compilação — o passo seguinte
/// simplesmente morre num erro de parser, na mão de quem obedeceu a resposta.
#[test]
fn o_proximo_passo_so_nomeia_comando_publicado() {
    let publicados = published_names();
    let mut orfaos = Vec::new();
    for (fase, comando) in NEXT_BY_PHASE {
        if !publicados.contains(&(*comando).to_string()) {
            orfaos.push(format!("a fase `{fase}` manda rodar `{comando}`, que não é publicado"));
        }
    }
    assert!(
        orfaos.is_empty(),
        "o campo de próximo passo aponta comando que a superfície não tem:\n{}",
        orfaos.join("\n")
    );
    assert!(!NEXT_BY_PHASE.is_empty(), "a tabela do próximo passo está vazia");
}

/// A resposta de verdade traz os três campos, e o comando que ela nomeia é o
/// que a tabela declara para aquela fase.
///
/// Prova o campo pela resposta, não pela tabela: é a resposta que chega a quem
/// conduz a conversa, e uma tabela certa com um relatório que não a usa seria
/// exatamente o defeito que este teste existe para pegar.
#[test]
fn a_resposta_do_passo_traz_a_fase_o_passo_em_palavras_e_o_comando() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);

    let aberta = run(root, &["open", "--kind", "feature", "--name", "unidade-de-teste", "--base", "dev"]);
    assert_eq!(aberta["ok"], serde_json::json!(true), "a abertura falhou: {aberta}");
    let spec = aberta["spec"].as_str().expect("a abertura nomeia a spec").to_string();

    let retomada = run(root, &["resume", "--spec", &spec]);
    assert_eq!(retomada["ok"], serde_json::json!(true), "{retomada}");
    let fase = retomada["phase"].as_str().expect("a resposta traz a fase");
    assert!(
        retomada["next"].as_str().is_some_and(|t| !t.trim().is_empty()),
        "a resposta não diz o próximo passo em palavras: {retomada}"
    );

    let esperado = NEXT_BY_PHASE
        .iter()
        .find(|(f, _)| *f == fase)
        .map(|(_, nome)| *nome)
        .unwrap_or_else(|| panic!("a fase `{fase}` não está na tabela do próximo passo"));
    let comando = retomada["command"].as_str().unwrap_or_default();
    assert!(
        comando.starts_with(&format!("mustard-rt run {esperado} ")),
        "a resposta manda rodar outra coisa que não `{esperado}`: {retomada}"
    );
}

/// A porta que o usuário tem manda repassar o campo `command` e proíbe
/// escolher o passo por conta própria.
///
/// Confere o fato, não a frase: a porta tem de nomear o campo e dizer que a
/// fase decide. Sem isso, a prosa volta a ensinar a ordem, que é o que a
/// resposta do comando substituiu.
#[test]
fn a_porta_repassa_o_campo_e_nao_escolhe_o_passo() {
    let texto = fs::read_to_string(repo_root().join("plugin/commands/continue.md"))
        .expect("a porta da retomada está entregue");
    assert!(
        texto.contains("`command`"),
        "a porta não nomeia o campo do próximo passo"
    );
    assert!(
        texto.contains("Never decide the next step yourself"),
        "a porta não proíbe escolher o passo por conta própria"
    );
    assert!(
        !texto.contains("mustard-rt run qa-run"),
        "a porta ainda manda rodar um comando que saiu"
    );
}
