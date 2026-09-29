//! A história do git que o mapa guarda vem da branch de partida que o
//! projeto declara no `mustard.json`, nunca da branch em que se está: o
//! commit que só a branch de trabalho tem fica de fora até o merge, e cada
//! commit guarda o número do pull request que o trouxe, quando o git o diz.
//! Os projetos são repositórios de verdade, numa pasta temporária.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::Command;

use mustard_core::domain::project_map::{examples, summary, FileLineage};
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
    assert!(!model::is_behind(dir));
    git(dir, &["update-ref", "refs/remotes/origin/main", newer.trim()]);
    assert_eq!(git(dir, &["rev-parse", "HEAD"]), checked_out, "the checkout stays as it was");
    assert!(model::is_behind(dir), "the base that moved puts the map behind");
    assert_eq!(
        commits(&scan(dir)),
        titled(&[("primeiro", None), ("entrou no servidor", None), ("mais um no servidor", None)])
    );
    assert!(!model::is_behind(dir));
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

/// Roda a passada da história de `file` sobre o mapa do projeto e devolve a
/// lista gravada dele.
fn lineage(dir: &Path, file: &str) -> FileLineage {
    lineage_with(dir, file, &[])
}

/// A passada de [`lineage`] com as opções `extra` a mais.
fn lineage_with(dir: &Path, file: &str, extra: &[&str]) -> FileLineage {
    let model = model::path_in(&dir.join(".claude"));
    let run = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["history", dir.to_str().unwrap(), "--out", model.to_str().unwrap(), "--file", file, "--json"])
        .args(extra)
        .output()
        .expect("run scan history");
    assert!(run.status.success(), "stderr: {}", String::from_utf8_lossy(&run.stderr));
    let report: Value = serde_json::from_str(String::from_utf8_lossy(&run.stdout).lines().last().unwrap_or("{}")).unwrap();
    assert_eq!(report["file"], json!(file), "{report}");
    let map = store::read_at(&model).expect("o mapa se lê");
    map.lineage.into_iter().find(|found| found.path == file).expect("a passada gravou a lista do arquivo")
}

/// Os commits da declaração `name` na lista, do mais novo ao mais velho,
/// cada um pelo título, com a marca de só forma.
fn changes(lineage: &FileLineage, name: &str) -> Vec<(String, bool)> {
    let decl = lineage.declarations.iter().find(|decl| decl.name == name).expect("a declaração está na lista");
    decl.commits
        .iter()
        .map(|change| {
            let commit = lineage.commits.iter().find(|commit| commit.id == change.id).expect("o commit está na lista");
            (commit.title.clone(), change.form)
        })
        .collect()
}

fn listed(titles: &[&str]) -> Vec<(String, bool)> {
    titles.iter().map(|title| ((*title).to_string(), false)).collect()
}

#[test]
fn a_function_changed_twice_lists_both_newest_first_and_the_one_below_keeps_its_own() {
    let temp = project("scan-linhagem-duas-");
    let dir = temp.path();
    declare_base(dir, "main");
    let two = |top: u32| format!("pub fn top() -> u32 {{\n    {top}\n}}\n\npub fn bottom() -> u32 {{\n    10\n}}\n");
    commit(dir, "src/conta.rs", &two(1), "cria as duas");
    commit(dir, "src/conta.rs", &two(2), "muda a de cima");
    commit(dir, "src/conta.rs", &two(3), "muda a de cima de novo");
    scan(dir);

    let found = lineage(dir, "src/conta.rs");
    assert_eq!(changes(&found, "top"), listed(&["muda a de cima de novo", "muda a de cima", "cria as duas"]));
    assert_eq!(changes(&found, "bottom"), listed(&["cria as duas"]), "a change only in the function above stays out of the one below");
    assert_eq!(found.base, "main");
}

#[test]
fn a_function_moved_to_another_file_keeps_the_commit_from_before_the_move() {
    let temp = project("scan-linhagem-movida-");
    let dir = temp.path();
    declare_base(dir, "main");
    let rest = "pub fn outra() -> u32 {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    a + b + c\n}\n";
    let ler = |n: u32| format!("pub fn ler(x: u32) -> u32 {{\n    let lido = x + {n};\n    lido * 2\n}}\n");
    commit(dir, "src/origem.rs", &format!("{}\n{rest}", ler(1)), "cria o ler");
    commit(dir, "src/origem.rs", &format!("{}\n{rest}", ler(2)), "muda o ler");
    write(dir, "src/origem.rs", rest);
    commit(dir, "src/destino.rs", &ler(2), "move o ler");
    scan(dir);

    let found = lineage(dir, "src/destino.rs");
    assert_eq!(
        changes(&found, "ler"),
        listed(&["muda o ler", "cria o ler"]),
        "the move with the same body keeps the history of the other file and is not a change of the function"
    );
}

/// A função movida duas vezes de arquivo: sem o número, a passada segue as
/// duas mudanças até o arquivo onde ela nasceu; com `--moves 1`, segue só a
/// última, e a lista guarda o número que seguiu.
#[test]
fn the_moves_number_limits_how_many_file_moves_the_history_follows() {
    let temp = project("scan-linhagem-mudancas-");
    let dir = temp.path();
    declare_base(dir, "main");
    let rest = "pub fn outra() -> u32 {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    a + b + c\n}\n";
    let rest2 = "pub fn mais() -> u32 {\n    let d = 4;\n    let e = 5;\n    let f = 6;\n    d * e * f\n}\n";
    let ler = |n: u32| format!("pub fn ler(x: u32) -> u32 {{\n    let lido = x + {n};\n    lido * 2\n}}\n");
    commit(dir, "src/origem.rs", &format!("{}\n{rest}", ler(1)), "cria o ler");
    commit(dir, "src/origem.rs", &format!("{}\n{rest}", ler(2)), "muda o ler");
    write(dir, "src/origem.rs", rest);
    commit(dir, "src/meio.rs", &format!("{}\n{rest2}", ler(2)), "move o ler para o meio");
    commit(dir, "src/meio.rs", &format!("{}\n{rest2}", ler(3)), "muda o ler no meio");
    write(dir, "src/meio.rs", rest2);
    commit(dir, "src/destino.rs", &ler(3), "move o ler para o destino");
    scan(dir);

    let every = lineage(dir, "src/destino.rs");
    assert_eq!(changes(&every, "ler"), listed(&["muda o ler no meio", "muda o ler", "cria o ler"]));
    assert_eq!(every.moves, 5, "sem o número, vale o padrão");

    let one = lineage_with(dir, "src/destino.rs", &["--moves", "1"]);
    assert_eq!(changes(&one, "ler"), listed(&["muda o ler no meio", "move o ler para o meio"]));
    assert_eq!(one.moves, 1);
}

#[test]
fn a_function_renamed_in_the_same_file_with_half_its_lines_keeps_the_history() {
    let temp = project("scan-linhagem-renomeada-");
    let dir = temp.path();
    declare_base(dir, "main");
    commit(
        dir,
        "src/nome.rs",
        "pub fn antigo(x: u32) -> u32 {\n    let dobro = x * 2;\n    let triplo = x * 3;\n    dobro + triplo\n}\n",
        "cria o antigo",
    );
    commit(
        dir,
        "src/nome.rs",
        "pub fn novo(x: u32) -> u32 {\n    let dobro = x * 2;\n    let triplo = x * 3;\n    dobro + triplo + 1\n}\n",
        "renomeia para novo",
    );
    scan(dir);

    let found = lineage(dir, "src/nome.rs");
    assert_eq!(changes(&found, "novo"), listed(&["renomeia para novo", "cria o antigo"]));
}

#[test]
fn a_renamed_file_takes_its_functions_along() {
    let temp = project("scan-linhagem-arquivo-");
    let dir = temp.path();
    declare_base(dir, "main");
    let junta = |n: u32| format!("pub fn junta(a: u32, b: u32) -> u32 {{\n    let soma = a + b;\n    soma + {n}\n}}\n");
    commit(dir, "src/velho.rs", &junta(1), "cria o junta");
    commit(dir, "src/velho.rs", &junta(2), "muda o junta");
    git(dir, &["mv", "src/velho.rs", "src/novo.rs"]);
    git(dir, &["commit", "-q", "-m", "renomeia o arquivo"]);
    scan(dir);

    let found = lineage(dir, "src/novo.rs");
    assert_eq!(changes(&found, "junta"), listed(&["muda o junta", "cria o junta"]));
}

#[test]
fn a_whitespace_commit_and_an_ignored_commit_are_marked_format_only() {
    let temp = project("scan-linhagem-forma-");
    let dir = temp.path();
    declare_base(dir, "main");
    commit(dir, "src/forma.rs", "pub fn calcula(x: u32) -> u32 {\n    x + 1\n}\n", "cria o calcula");
    commit(dir, "src/forma.rs", "pub fn calcula(x: u32) -> u32 {\n    x + 2\n}\n", "muda o calcula");
    commit(dir, "src/forma.rs", "pub fn calcula(x: u32) -> u32 {\n        x  +  2\n}\n", "só espaços");
    commit(dir, "src/forma.rs", "pub fn calcula(x: u32) -> u32 {\n    (x + 2)\n}\n", "formata");
    let formatted = git(dir, &["rev-parse", "HEAD"]);
    write(dir, ".git-blame-ignore-revs", &format!("# a formatação do projeto\n{formatted}"));
    scan(dir);

    let found = lineage(dir, "src/forma.rs");
    assert_eq!(
        changes(&found, "calcula"),
        vec![
            ("formata".to_string(), true),
            ("só espaços".to_string(), true),
            ("muda o calcula".to_string(), false),
            ("cria o calcula".to_string(), false),
        ]
    );
}

#[test]
fn a_file_older_than_the_window_gets_its_old_history_with_the_number_and_the_others_stay() {
    let temp = tempfile::Builder::new().prefix("scan-linhagem-antiga-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    declare_base(dir, "main");
    let data = |text: &str| format!("data {}\n{text}\n", text.len());
    let mut stream = String::new();
    let mut at = 1_000_000_000u64;
    let mut push = |stream: &mut String, title: &str, files: &[(&str, String)]| {
        at += 1;
        stream.push_str(&format!("commit refs/heads/main\ncommitter scan <scan@example.com> {at} +0000\n"));
        stream.push_str(&data(title));
        for (path, body) in files {
            stream.push_str(&format!("M 100644 inline {path}\n"));
            stream.push_str(&data(body));
        }
    };
    push(&mut stream, "cria a antiga (#12)", &[("src/antiga.rs", "pub fn antiga() -> u32 {\n    7\n}\n".to_string())]);
    let window = mustard_core::domain::project_map::MAX_COMMITS;
    for i in 0..=window {
        push(&mut stream, &format!("ruido {i}"), &[("src/ruido.rs", format!("pub const N: u32 = {i};\n"))]);
    }
    push(&mut stream, "cria o outro", &[("src/outro.rs", "pub fn outro() -> u32 {\n    1\n}\n".to_string())]);
    let mut import = Command::new("git").args(["fast-import", "--quiet"]).current_dir(dir).stdin(std::process::Stdio::piped()).spawn().unwrap();
    std::io::Write::write_all(import.stdin.as_mut().unwrap(), stream.as_bytes()).unwrap();
    assert!(import.wait().unwrap().success());
    git(dir, &["reset", "-q", "--hard"]);
    let map = scan(dir);
    assert!(
        !commits(&map).iter().any(|(title, _)| title.starts_with("cria a antiga")),
        "the creation of the old file is beyond the window the map keeps"
    );

    let other = lineage(dir, "src/outro.rs");
    let old = lineage(dir, "src/antiga.rs");
    assert_eq!(changes(&old, "antiga"), listed(&["cria a antiga (#12)"]));
    assert_eq!(old.commits.iter().map(|commit| commit.pr).collect::<Vec<_>>(), vec![Some(12)]);
    let map = store::read_at(&model::path_in(&dir.join(".claude"))).unwrap();
    assert_eq!(map.lineage.iter().find(|found| found.path == "src/outro.rs"), Some(&other), "the list of another file stays as it was");
}

/// Cada commit da história de um arquivo guarda os arquivos que ele criou e
/// mudou, o próprio incluído: é deles que sai a receita do arquivo cujo
/// último commit ficou fora da janela do mapa.
#[test]
fn each_commit_of_a_file_history_keeps_the_files_it_created_and_changed() {
    use mustard_core::domain::project_map::CommitFiles;

    let temp = project("scan-linhagem-arquivos-");
    let dir = temp.path();
    declare_base(dir, "main");
    write(dir, "src/registro.rs", "pub mod pagar;\n");
    commit(dir, "src/pagar.rs", "pub fn pagar() -> u32 {\n    1\n}\n", "cria o pagar");
    write(dir, "tests/pagar.rs", "#[test]\nfn paga() {}\n");
    commit(dir, "src/pagar.rs", "pub fn pagar() -> u32 {\n    2\n}\n", "muda o pagar");
    scan(dir);

    let found = lineage(dir, "src/pagar.rs");
    // Os dois commits podem cair no mesmo segundo: a ordem entre eles não
    // conta aqui.
    let mut files: Vec<(&str, CommitFiles)> = found.commits.iter().map(|c| (c.title.as_str(), c.files.clone())).collect();
    files.sort_by_key(|(title, _)| *title);
    let listed = |added: &[&str], changed: &[&str]| CommitFiles {
        added: added.iter().map(|p| (*p).to_string()).collect(),
        changed: changed.iter().map(|p| (*p).to_string()).collect(),
    };
    assert_eq!(
        files,
        vec![
            ("cria o pagar", listed(&["src/pagar.rs", "src/registro.rs"], &[])),
            ("muda o pagar", listed(&["tests/pagar.rs"], &["src/pagar.rs"])),
        ],
    );
}

#[test]
fn a_change_in_the_comment_right_above_a_function_belongs_to_it() {
    let temp = project("scan-linhagem-comentario-");
    let dir = temp.path();
    declare_base(dir, "main");
    let body = |doc: &str| format!("pub fn antes() {{}}\n\n/// {doc}\npub fn soma() -> u32 {{\n    2\n}}\n");
    commit(dir, "src/doc.rs", &body("Soma."), "cria a soma");
    commit(dir, "src/doc.rs", &body("Soma dois."), "explica a soma");
    scan(dir);

    let found = lineage(dir, "src/doc.rs");
    assert_eq!(changes(&found, "soma"), listed(&["explica a soma", "cria a soma"]));
    assert_eq!(changes(&found, "antes"), listed(&["cria a soma"]));
}

/// Um comentário de revisão preso a uma linha cai na função que continha a
/// linha no commit comentado, mesmo que ela tenha descido no arquivo depois;
/// o preso a uma linha fora de qualquer função fica sem função; e o do
/// commit que o clone não tem, o do ramo apagado depois de um squash, cai
/// pelas linhas do commit da base com o número do pull request.
#[test]
fn a_review_comment_joins_the_function_that_held_its_line_in_the_commented_commit() {
    use mustard_core::domain::project_map::{PullComment, PullText};

    let temp = project("scan-linhagem-revisao-");
    let dir = temp.path();
    declare_base(dir, "main");
    let two = "pub fn top() -> u32 {\n    1\n}\n\npub fn bottom() -> u32 {\n    10\n}\n";
    commit(dir, "src/conta.rs", two, "cria as duas (#3)");
    let created = git(dir, &["rev-parse", "HEAD"]).trim().to_string();
    commit(dir, "src/conta.rs", &format!("pub fn novo() {{}}\n\n{two}"), "põe o novo em cima (#4)");
    scan(dir);

    let model = model::path_in(&dir.join(".claude"));
    let text = |number: u32| PullText { number, title: format!("pull request {number}"), ..PullText::default() };
    let comment = |number: u32, commit: &str, line: u64, body: &str| PullComment {
        number,
        commit: commit.to_string(),
        path: "src/conta.rs".to_string(),
        line,
        body: body.to_string(),
    };
    store::save_pull_at(&model, &text(3), &[comment(3, &created, 6, "e o dez?"), comment(3, &created, 4, "linha em branco")]).unwrap();
    let gone = "0123456789abcdef0123456789abcdef01234567";
    store::save_pull_at(&model, &text(4), &[comment(4, gone, 1, "nome melhor")]).unwrap();

    let found = lineage(dir, "src/conta.rs");
    let bodies = |name: &str| -> Vec<(u32, String, String)> {
        let decl = found.declarations.iter().find(|decl| decl.name == name).expect("a declaração está na lista");
        decl.comments.iter().map(|c| (c.pr, c.commit.clone(), c.body.clone())).collect()
    };
    assert_eq!(bodies("bottom"), [(3, created[..10].to_string(), "e o dez?".to_string())], "{found:?}");
    assert_eq!(bodies("novo"), [(4, gone[..10].to_string(), "nome melhor".to_string())], "{found:?}");
    assert!(bodies("top").is_empty(), "{found:?}");
    assert_eq!(found.comments, 3, "a lista guarda quantos comentários presos ao arquivo o mapa tinha");
}
