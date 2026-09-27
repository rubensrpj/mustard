//! A história do git que o mapa guarda vem da branch de partida que o
//! projeto declara no `mustard.json`, nunca da branch em que se está: o
//! commit que só a branch de trabalho tem fica de fora até o merge, e cada
//! commit guarda o número do pull request que o trouxe, quando o git o diz.
//! Os projetos são repositórios de verdade, numa pasta temporária.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::Command;

use mustard_core::domain::project_map::{examples, summary};
use mustard_core::io::project_map as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// Escreve no `mustard.json` a branch de partida `base`, na chave `*` do
/// fluxo do git.
fn declare_base(dir: &Path, base: &str) {
    write(dir, "mustard.json", &json!({"git": {"flow": {"*": base}}}).to_string());
}

/// Um projeto no git, na branch `main`, com as regras que a instalação do
/// Mustard escreve para o mapa e o `mustard.json` ficarem fora do git, e o
/// primeiro commit.
fn project(prefix: &str) -> tempfile::TempDir {
    let temp = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    write(dir, "src/a.rs", "pub fn alpha() {}\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    temp
}

/// Grava `body` em `rel` e comita com o título `title`.
fn commit(dir: &Path, rel: &str, body: &str, title: &str) {
    write(dir, rel, body);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", title]);
}

/// Roda o scan com o mapa dentro do projeto e devolve o mapa gravado.
fn scan(dir: &Path) -> Value {
    model::scan(dir, &dir.join(".claude"), &[]).0
}

/// Os commits da história gravada, do mais antigo para o mais novo, cada um
/// com o título e o número do pull request.
fn commits(map: &Value) -> Vec<(String, Option<u64>)> {
    map["history"]["commits"]
        .as_array()
        .map(|all| {
            all.iter().map(|c| (c["title"].as_str().unwrap_or_default().to_string(), c["pr"].as_u64())).collect()
        })
        .unwrap_or_default()
}

fn titled(list: &[(&str, Option<u64>)]) -> Vec<(String, Option<u64>)> {
    list.iter().map(|(title, pr)| ((*title).to_string(), *pr)).collect()
}

#[test]
fn a_commit_only_on_the_work_branch_stays_out_until_the_merge_brings_it_with_the_number() {
    let temp = project("scan-historia-merge-");
    let dir = temp.path();
    declare_base(dir, "main");
    git(dir, &["checkout", "-q", "-b", "trabalho"]);
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "adiciona o beta");

    let before = scan(dir);
    assert_eq!(before["history"]["base"], json!("main"), "{}", before["history"]);
    assert_eq!(commits(&before), titled(&[("primeiro", None)]), "the work branch commit is not in the history");

    git(dir, &["checkout", "-q", "main"]);
    git(dir, &["merge", "-q", "--no-ff", "-m", "Merge pull request #7 from time/trabalho", "trabalho"]);
    git(dir, &["checkout", "-q", "trabalho"]);
    let after = scan(dir);
    assert_eq!(
        commits(&after),
        titled(&[("primeiro", None), ("adiciona o beta", Some(7))]),
        "the merge brings the commit with its number, and the merge itself is not kept"
    );

    // A soma do que é novo dá a mesma história que a leitura inteira.
    let stepped = model::read_bytes(&dir.join(".claude"));
    model::scan(dir, &dir.join(".claude"), &["--all"]);
    assert_eq!(model::read_bytes(&dir.join(".claude")), stepped);
}

#[test]
fn the_star_of_the_flow_picks_the_branch_the_history_comes_from() {
    let temp = project("scan-historia-flow-");
    let dir = temp.path();
    git(dir, &["checkout", "-q", "-b", "develop"]);
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "só no develop");
    git(dir, &["checkout", "-q", "main"]);

    declare_base(dir, "main");
    assert_eq!(commits(&scan(dir)), titled(&[("primeiro", None)]));

    declare_base(dir, "develop");
    let develop = scan(dir);
    assert_eq!(develop["history"]["base"], json!("develop"));
    assert_eq!(commits(&develop), titled(&[("primeiro", None), ("só no develop", None)]), "no code changed, only the flow");
}

#[test]
fn the_history_comes_from_the_server_tip_the_clone_has_before_the_local_branch() {
    let temp = project("scan-historia-origin-");
    let dir = temp.path();
    declare_base(dir, "main");
    git(dir, &["checkout", "-q", "-b", "trabalho"]);
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "entrou no servidor");
    let server = git(dir, &["rev-parse", "HEAD"]);
    git(dir, &["checkout", "-q", "main"]);
    git(dir, &["update-ref", "refs/remotes/origin/main", server.trim()]);
    assert_eq!(commits(&scan(dir)), titled(&[("primeiro", None), ("entrou no servidor", None)]));

    // O servidor andou sem mudar nada no checkout: a passada seguinte soma
    // o commit novo.
    git(dir, &["checkout", "-q", "trabalho"]);
    commit(dir, "src/c.rs", "pub fn gamma() {}\n", "mais um no servidor");
    let newer = git(dir, &["rev-parse", "HEAD"]);
    git(dir, &["checkout", "-q", "main"]);
    let checked_out = git(dir, &["rev-parse", "HEAD"]);
    scan(dir);
    assert!(!store::is_behind(dir));
    git(dir, &["update-ref", "refs/remotes/origin/main", newer.trim()]);
    assert_eq!(git(dir, &["rev-parse", "HEAD"]), checked_out, "the checkout stays as it was");
    assert!(store::is_behind(dir), "the base that moved puts the map behind");
    assert_eq!(
        commits(&scan(dir)),
        titled(&[("primeiro", None), ("entrou no servidor", None), ("mais um no servidor", None)])
    );
    assert!(!store::is_behind(dir));
}

#[test]
fn a_rewritten_base_reads_the_window_again() {
    let temp = project("scan-historia-reescrita-");
    let dir = temp.path();
    declare_base(dir, "main");
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "vai sumir");
    assert_eq!(commits(&scan(dir)).len(), 2);
    git(dir, &["reset", "-q", "--hard", "HEAD~1"]);
    commit(dir, "src/c.rs", "pub fn gamma() {}\n", "ficou no lugar");
    assert_eq!(commits(&scan(dir)), titled(&[("primeiro", None), ("ficou no lugar", None)]));
}

#[test]
fn a_squash_title_ending_in_the_number_keeps_it() {
    let temp = project("scan-historia-squash-");
    let dir = temp.path();
    declare_base(dir, "main");
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "junta a busca (#12)");
    commit(dir, "src/c.rs", "pub fn gamma() {}\n", "cita o (#12) no meio");
    assert_eq!(
        commits(&scan(dir)),
        titled(&[("primeiro", None), ("junta a busca (#12)", Some(12)), ("cita o (#12) no meio", None)])
    );
}

#[test]
fn a_project_without_a_flow_scans_and_says_there_is_no_base() {
    let temp = project("scan-historia-sem-base-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "segundo");
    let map = scan(dir);
    assert_eq!(commits(&map), Vec::new(), "no base, no history");
    assert_eq!(map["history"]["missing"], json!("no_base"), "{}", map["history"]);
    assert!(map["modules"].as_array().is_some_and(|all| all.len() == 2), "the rest of the map is there");

    let read = store::read(dir).expect("o mapa foi gravado");
    for lang in [Locale::PtBr, Locale::EnUs] {
        let note = translate("map.history.no_base", lang);
        assert!(summary(&read, lang).contains(note), "{lang:?}: {}", summary(&read, lang));
        assert_eq!(examples(&read, "src/c.rs", lang).no_history.as_deref(), Some(note), "{lang:?}");
    }

    // A base declarada que o clone não tem também diz por quê.
    declare_base(dir, "dev");
    let map = scan(dir);
    assert_eq!((map["history"]["base"].clone(), map["history"]["missing"].clone()), (json!("dev"), json!("base_not_found")));
    let read = store::read(dir).expect("o mapa foi gravado");
    let note = translate("map.history.base_not_found", Locale::PtBr).replace("{base}", "dev");
    assert!(summary(&read, Locale::PtBr).contains(&note), "{}", summary(&read, Locale::PtBr));
}
