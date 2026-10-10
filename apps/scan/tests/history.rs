//! A história do git que o mapa guarda vem da branch de partida que o
//! projeto declara no `mustard.json` e, sem declaração, da branch padrão do
//! servidor e, sem servidor, da branch do checkout: o commit que só a branch
//! de trabalho tem fica de fora até o merge, e cada commit guarda o número do
//! pull request que o trouxe, quando o git o diz.
//! Os projetos são repositórios de verdade, numa pasta temporária.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::{Command, Stdio};

use mustard_core::domain::config::ProjectConfig;
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::project_map::{FileLineage,examples, summary};
use mustard_core::io::map_search::{any_path, candidates_at};
use mustard_core::io::map_triage::triage_at;
use mustard_core::io::project_map as store;
use mustard_core::platform::i18n::{Locale,translate};
use serde_json::{Value,json};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false"])
        .args(args)
        // Todos os commits nascem no mesmo segundo: a ordem entre eles só se
        // sabe pelo lugar que têm na história, como num rebase.
        .env("GIT_AUTHOR_DATE", "2026-01-01T12:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T12:00:00Z")
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
    let temp = project("scan-history-merge-");
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
    let temp = project("scan-history-flow-");
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
    let temp = project("scan-history-origin-");
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
    let temp = project("scan-history-rewrite-");
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
    let temp = project("scan-history-squash-");
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
fn a_project_without_a_flow_takes_the_branch_the_checkout_is_on() {
    let temp = project("scan-history-no-flow-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "segundo");
    let map = scan(dir);
    assert_eq!(map["history"]["base"], json!("main"), "{}", map["history"]);
    assert_eq!(commits(&map), titled(&[("primeiro", None), ("segundo", None)]), "the branch of the checkout gives the history");
    assert_eq!(map["history"]["missing"], Value::Null, "{}", map["history"]);
    assert!(map["modules"].as_array().is_some_and(|all| all.len() == 2), "the rest of the map is there");
}

#[test]
fn a_project_without_a_flow_takes_the_default_branch_of_the_server_before_the_checkout() {
    let temp = project("scan-history-server-");
    let dir = temp.path();
    git(dir, &["checkout", "-q", "-b", "develop"]);
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "entrou no develop");
    let develop = git(dir, &["rev-parse", "HEAD"]);
    git(dir, &["checkout", "-q", "-b", "trabalho"]);
    commit(dir, "src/c.rs", "pub fn gama() {}\n", "só no trabalho");
    git(dir, &["update-ref", "refs/remotes/origin/develop", develop.trim()]);
    git(dir, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/develop"]);

    let map = scan(dir);
    assert_eq!(map["history"]["base"], json!("develop"), "{}", map["history"]);
    assert_eq!(
        commits(&map),
        titled(&[("primeiro", None), ("entrou no develop", None)]),
        "the server default branch gives the history, and the checkout commit stays out"
    );
    assert!(!dir.join("mustard.json").exists(), "the configuration is only read, never written");
}

#[test]
fn a_project_with_no_branch_to_read_scans_and_says_there_is_no_base() {
    let temp = project("scan-history-no-base-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "segundo");
    // O checkout solto de branch, sem servidor: nenhuma branch a ler.
    git(dir, &["checkout", "-q", "--detach"]);
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
    history_in(dir, &dir.join(".claude"), file, extra).1
}

/// A passada da história de `file` sobre o mapa da pasta `out`, do projeto em
/// `dir`, com `extra` a mais: o relato dela e a lista gravada do arquivo.
fn history_in(dir: &Path, out: &Path, file: &str, extra: &[&str]) -> (Value, FileLineage) {
    let model = model::path_in(out);
    let run = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["history", dir.to_str().unwrap(), "--out", model.to_str().unwrap(), "--file", file, "--json"])
        .args(extra)
        .output()
        .expect("run scan history");
    assert!(run.status.success(), "stderr: {}", String::from_utf8_lossy(&run.stderr));
    let report: Value = serde_json::from_str(String::from_utf8_lossy(&run.stdout).lines().last().unwrap_or("{}")).unwrap();
    assert_eq!(report["file"], json!(file), "{report}");
    let map = store::read_at(&model).expect("o mapa se lê");
    let found = map.lineage.into_iter().find(|found| found.path == file).expect("a passada gravou a lista do arquivo");
    (report, found)
}

/// A lista de `file` como a leitura do começo da história a dá: num mapa novo,
/// que ainda não guarda lista nenhuma.
fn from_the_start(dir: &Path, file: &str) -> FileLineage {
    let out = tempfile::tempdir().unwrap();
    model::scan(dir, out.path(), &[]);
    history_in(dir, out.path(), file, &[]).1
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

/// Os commits da `nth`-ésima declaração de nome `name` do arquivo, como em
/// [`changes`].
fn changes_at(lineage: &FileLineage, name: &str, nth: u32) -> Vec<(String, bool)> {
    let decl = lineage.declarations.iter().find(|decl| decl.name == name && decl.nth == nth).expect("a declaração está na lista");
    decl.commits
        .iter()
        .map(|change| {
            let commit = lineage.commits.iter().find(|commit| commit.id == change.id).expect("o commit está na lista");
            (commit.title.clone(), change.form)
        })
        .collect()
}

#[test]
fn a_function_changed_twice_lists_both_newest_first_and_the_one_below_keeps_its_own() {
    let temp = project("scan-lineage-two-");
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

/// Dois campos de mesmo nome em tipos diferentes: o que ganha um irmão de nome
/// igual acima dele não perde o próprio passado, e o novo não herda o do outro.
#[test]
fn a_declaration_keeps_its_own_history_when_another_of_the_same_name_is_added_above_it() {
    let temp = project("scan-lineage-same-name-above-");
    let dir = temp.path();
    declare_base(dir, "main");
    let old = |width: &str| format!("pub struct Old {{\n    pub id: {width},\n}}\n");
    let new = "pub struct New {\n    pub id: u32,\n}\n\n";
    commit(dir, "src/campos.rs", &old("u32"), "cria a antiga");
    commit(dir, "src/campos.rs", &format!("{new}{}", old("u32")), "põe a nova em cima");
    commit(dir, "src/campos.rs", &format!("{new}{}", old("u64")), "alarga o campo da antiga");
    scan(dir);

    let found = lineage(dir, "src/campos.rs");
    assert_eq!(changes(&found, "New"), listed(&["põe a nova em cima"]), "{found:?}");
    assert_eq!(changes_at(&found, "id", 0), listed(&["põe a nova em cima"]), "the new field is born in the commit that put it there: {found:?}");
    assert_eq!(changes(&found, "Old"), listed(&["cria a antiga"]), "the change inside the field is the field's, not the type's: {found:?}");
    assert_eq!(
        changes_at(&found, "id", 1),
        listed(&["alarga o campo da antiga", "cria a antiga"]),
        "the old field keeps its own commits, from before the other one existed: {found:?}"
    );
}

/// A que fica quando a de mesmo nome acima dela sai não leva o passado da que
/// saiu nem o commit que a tirou.
#[test]
fn a_declaration_keeps_its_own_history_when_another_of_the_same_name_above_it_is_removed() {
    let temp = project("scan-lineage-same-name-removed-");
    let dir = temp.path();
    declare_base(dir, "main");
    let old = "pub struct Old {\n    pub id: u32,\n}\n";
    commit(dir, "src/campos.rs", old, "cria a antiga");
    commit(dir, "src/campos.rs", &format!("pub struct New {{\n    pub id: u32,\n}}\n\n{old}"), "põe a nova em cima");
    commit(dir, "src/campos.rs", old, "tira a nova");
    scan(dir);

    let found = lineage(dir, "src/campos.rs");
    assert_eq!(changes_at(&found, "id", 0), listed(&["cria a antiga"]), "only the commit that made the field: {found:?}");
    assert_eq!(changes(&found, "Old"), listed(&["cria a antiga"]), "{found:?}");
}

/// O campo é a linha que o diff dá por igual: a mesma linha era o parâmetro de
/// uma função, que sumiu. A linha foi escrita no primeiro commit e é dele; o
/// tipo, que junta as linhas dos dois commits, lista os dois.
#[test]
fn a_declaration_made_of_a_line_the_diff_calls_unchanged_keeps_the_commit_that_wrote_the_line() {
    let temp = project("scan-lineage-born-of-unchanged-line-");
    let dir = temp.path();
    declare_base(dir, "main");
    commit(dir, "src/pedido.rs", "pub fn one(\n    root: &str,\n    lang: u32,\n) -> u32 {\n    lang\n}\n", "a função recebe os dois");
    commit(
        dir,
        "src/pedido.rs",
        "pub struct Context {\n    root: &str,\n    lang: u32,\n}\n\npub fn one(context: &Context) -> u32 {\n    context.lang\n}\n",
        "os dois viram um tipo",
    );
    scan(dir);

    let found = lineage(dir, "src/pedido.rs");
    assert_eq!(changes(&found, "lang"), listed(&["a função recebe os dois"]), "{found:?}");
    assert_eq!(changes(&found, "root"), listed(&["a função recebe os dois"]), "{found:?}");
    assert_eq!(changes(&found, "Context"), listed(&["os dois viram um tipo", "a função recebe os dois"]), "{found:?}");
}

#[test]
fn a_function_moved_to_another_file_keeps_the_commit_from_before_the_move() {
    let temp = project("scan-lineage-moved-");
    let dir = temp.path();
    declare_base(dir, "main");
    let rest = "pub fn outra() -> u32 {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    a + b + c\n}\n";
    let read_source = |n: u32| format!("pub fn ler(x: u32) -> u32 {{\n    let lido = x + {n};\n    lido * 2\n}}\n");
    commit(dir, "src/origem.rs", &format!("{}\n{rest}", read_source(1)), "cria o ler");
    commit(dir, "src/origem.rs", &format!("{}\n{rest}", read_source(2)), "muda o ler");
    write(dir, "src/origem.rs", rest);
    commit(dir, "src/destino.rs", &read_source(2), "move o ler");
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
    let temp = project("scan-lineage-moves-");
    let dir = temp.path();
    declare_base(dir, "main");
    let rest = "pub fn outra() -> u32 {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    a + b + c\n}\n";
    let rest2 = "pub fn mais() -> u32 {\n    let d = 4;\n    let e = 5;\n    let f = 6;\n    d * e * f\n}\n";
    let read_source = |n: u32| format!("pub fn ler(x: u32) -> u32 {{\n    let lido = x + {n};\n    lido * 2\n}}\n");
    commit(dir, "src/origem.rs", &format!("{}\n{rest}", read_source(1)), "cria o ler");
    commit(dir, "src/origem.rs", &format!("{}\n{rest}", read_source(2)), "muda o ler");
    write(dir, "src/origem.rs", rest);
    commit(dir, "src/meio.rs", &format!("{}\n{rest2}", read_source(2)), "move o ler para o meio");
    commit(dir, "src/meio.rs", &format!("{}\n{rest2}", read_source(3)), "muda o ler no meio");
    write(dir, "src/meio.rs", rest2);
    commit(dir, "src/destino.rs", &read_source(3), "move o ler para o destino");
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
    let temp = project("scan-lineage-renamed-");
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
    let temp = project("scan-lineage-file-");
    let dir = temp.path();
    declare_base(dir, "main");
    let join_source = |n: u32| format!("pub fn junta(a: u32, b: u32) -> u32 {{\n    let soma = a + b;\n    soma + {n}\n}}\n");
    commit(dir, "src/velho.rs", &join_source(1), "cria o junta");
    commit(dir, "src/velho.rs", &join_source(2), "muda o junta");
    git(dir, &["mv", "src/velho.rs", "src/novo.rs"]);
    git(dir, &["commit", "-q", "-m", "renomeia o arquivo"]);
    scan(dir);

    let found = lineage(dir, "src/novo.rs");
    assert_eq!(changes(&found, "junta"), listed(&["muda o junta", "cria o junta"]));
}

#[test]
fn a_whitespace_commit_and_an_ignored_commit_are_marked_format_only() {
    let temp = project("scan-lineage-format-");
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
    let temp = tempfile::Builder::new().prefix("scan-lineage-old-").tempdir().unwrap();
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
    // A criação do arquivo velho está fora dos 5.000 commits que a leitura lê
    // por padrão: só um limite maior a alcança.
    let old = lineage_with(dir, "src/antiga.rs", &["--newest", "6000"]);
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

    let temp = project("scan-lineage-files-");
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
    let temp = project("scan-lineage-comment-");
    let dir = temp.path();
    declare_base(dir, "main");
    let body = |doc: &str| format!("pub fn antes() {{}}\n\n/// {doc}\npub fn sum() -> u32 {{\n    2\n}}\n");
    commit(dir, "src/doc.rs", &body("Soma."), "cria a soma");
    commit(dir, "src/doc.rs", &body("Soma dois."), "explica a soma");
    scan(dir);

    let found = lineage(dir, "src/doc.rs");
    assert_eq!(changes(&found, "sum"), listed(&["explica a soma", "cria a soma"]));
    assert_eq!(changes(&found, "antes"), listed(&["cria a soma"]));
}

/// Um comentário de revisão preso a uma linha cai na função que continha a
/// linha no commit comentado, mesmo que ela tenha descido no arquivo depois;
/// o preso a uma linha fora de qualquer função fica sem função; e o do
/// commit que o clone não tem, o do ramo apagado depois de um squash, cai
/// pelas linhas do commit da base com o número do pull request.
/// A variante de enumeração escrita com os campos na mesma linha ocupa
/// exatamente as mesmas linhas que eles: o commit que mexeu na linha é da
/// variante e de cada campo, e não só do último que o arquivo declara. A
/// enumeração, que tem linhas próprias, não recebe o commit que mexeu só na
/// linha da variante.
#[test]
fn every_declaration_written_on_the_same_line_gets_the_commit_that_touched_the_line() {
    let temp = project("scan-lineage-same-line-");
    let dir = temp.path();
    declare_base(dir, "main");
    let body = |reason: &str| {
        format!("pub enum Action {{\n    List,\n    Add {{ title: String, detail: String }},\n    Close {{ id: String, reason: {reason} }},\n}}\n")
    };
    commit(dir, "src/action.rs", &body("String"), "cria a ação");
    commit(dir, "src/action.rs", &body("Vec<u8>"), "muda o motivo");
    scan(dir);

    let found = lineage(dir, "src/action.rs");
    for name in ["Add", "title", "detail"] {
        assert_eq!(
            changes(&found, name),
            listed(&["cria a ação"]),
            "{name}: the line was written once"
        );
    }
    for name in ["Close", "id", "reason"] {
        assert_eq!(
            changes(&found, name),
            listed(&["muda o motivo", "cria a ação"]),
            "{name}: the line changed twice"
        );
    }
    assert_eq!(
        changes(&found, "Action"),
        listed(&["cria a ação"]),
        "the enum's own lines did not change"
    );
}

/// Um comentário de revisão preso a uma linha cai na função que continha a
/// linha no commit comentado, mesmo que ela tenha descido no arquivo depois;
/// o preso a uma linha fora de qualquer função fica sem função; e o do
/// commit que o clone não tem, o do ramo apagado depois de um squash, cai
/// pelas linhas do commit da base com o número do pull request.
#[test]
fn a_review_comment_joins_the_function_that_held_its_line_in_the_commented_commit() {
    use mustard_core::domain::project_map::{PullComment, PullText};

    let temp = project("scan-lineage-review-");
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

/// Roda a leitura da história de todo arquivo do mapa do projeto, com `extra`
/// a mais, e devolve o relato dela.
fn history_all(dir: &Path, extra: &[&str]) -> Value {
    history_all_in(dir, &dir.join(".claude"), extra)
}

/// A leitura de [`history_all`] sobre o mapa da pasta `out`.
fn history_all_in(dir: &Path, out: &Path, extra: &[&str]) -> Value {
    let model = model::path_in(out);
    let run = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["history-all", dir.to_str().unwrap(), "--out", model.to_str().unwrap(), "--json"])
        .args(extra)
        .output()
        .expect("run scan history-all");
    assert!(run.status.success(), "stderr: {}", String::from_utf8_lossy(&run.stderr));
    serde_json::from_str(String::from_utf8_lossy(&run.stdout).lines().last().unwrap_or("{}")).expect("o relato é uma linha JSON")
}

/// A lista da história gravada no mapa para `file`, quando o mapa a tem.
fn stored_lineage(dir: &Path, file: &str) -> Option<FileLineage> {
    let map = store::read_at(&model::path_in(&dir.join(".claude"))).expect("o mapa se lê");
    map.lineage.into_iter().find(|found| found.path == file)
}

#[test]
fn a_project_without_a_declared_base_gets_the_history_of_each_function() {
    let temp = project("scan-history-whole-no-base-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "cria o beta");
    commit(dir, "src/b.rs", "pub fn beta() { 1 }\n", "muda o beta");
    scan(dir);
    assert!(!dir.join("mustard.json").exists(), "the project declares no base");

    let report = history_all(dir, &[]);
    assert_eq!(report["files"], json!(2), "{report}");
    assert_eq!(report["failed"], json!(0), "{report}");

    let b = stored_lineage(dir, "src/b.rs").expect("the history of the file was written");
    assert_eq!(b.base, "main");
    assert_eq!(changes(&b, "beta"), listed(&["muda o beta", "cria o beta"]), "{b:?}");
    let a = stored_lineage(dir, "src/a.rs").expect("the history of the other file was written");
    assert_eq!(changes(&a, "alpha"), listed(&["primeiro"]), "{a:?}");
    assert!(!dir.join("mustard.json").exists(), "the configuration is only read, never written");
}

#[test]
fn a_project_with_no_branch_to_read_gets_no_history_from_the_whole_reading() {
    let temp = project("scan-history-whole-loose-");
    let dir = temp.path();
    git(dir, &["checkout", "-q", "--detach"]);
    scan(dir);

    let report = history_all(dir, &[]);
    assert_eq!(report["files"], json!(0), "{report}");
    assert!(stored_lineage(dir, "src/a.rs").is_none(), "there is no base to read the history from");
}

/// O vetor de sentido que o mapa do projeto guarda para a declaração `name`.
fn stored_vector(dir: &Path, name: &str) -> Vec<i8> {
    let conn = rusqlite::Connection::open(model::path_in(&dir.join(".claude"))).unwrap();
    let blob: Vec<u8> =
        conn.query_row("SELECT vector FROM decl_vectors WHERE name = ?1", [name], |row| row.get(0)).expect("the declaration has a vector");
    blob.iter().map(|byte| i8::from_le_bytes([*byte])).collect()
}

/// O resumo de cada declaração junta os títulos dos commits que a mudaram, e
/// essa história só chega com a leitura do projeto inteiro: ao fim dela, o
/// vetor se refaz só nas declarações cujo texto mudou. A função movida de
/// arquivo só tem o commit da mudança no arquivo novo; a história traz os
/// dois commits de antes, e o vetor passa a se parecer com o título deles.
/// A função cuja história já era a do arquivo não é refeita, e uma segunda
/// leitura, sem nada novo, não refaz vetor nenhum.
#[test]
fn the_whole_reading_redoes_the_vector_of_the_declarations_whose_history_it_brought() {
    use mustard_core::io::map_meaning::quantized_vector;

    /// O cosseno de dois vetores em int8: 1 para o mesmo sentido, 0 para nenhuma relação.
    fn cosine(a: &[i8], b: &[i8]) -> f32 {
        let dot: i32 = a.iter().zip(b).map(|(x, y)| i32::from(*x) * i32::from(*y)).sum();
        let norm = |v: &[i8]| f64::from(v.iter().map(|x| i32::from(*x) * i32::from(*x)).sum::<i32>()).sqrt();
        (f64::from(dot) / (norm(a) * norm(b))) as f32
    }

    let temp = project("scan-history-vector-");
    let dir = temp.path();
    declare_base(dir, "main");
    write(dir, "mustard.json", r#"{"ai":{"vectors":true},"git":{"flow":{"*":"main"}}}"#);
    let rest = "pub fn outra() -> u32 {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    a + b + c\n}\n";
    let read_source = |n: u32| format!("pub fn ler(x: u32) -> u32 {{\n    let lido = x + {n};\n    lido * 2\n}}\n");
    commit(dir, "src/origem.rs", &format!("{}\n{rest}", read_source(1)), "cria o ler");
    commit(dir, "src/origem.rs", &format!("{}\n{rest}", read_source(2)), "muda o ler");
    write(dir, "src/origem.rs", rest);
    commit(dir, "src/destino.rs", &read_source(2), "move o ler");
    scan(dir);
    let (read_before, alpha_before) = (stored_vector(dir, "ler"), stored_vector(dir, "alpha"));
    let earlier = quantized_vector("cria o ler muda o ler").expect("the meaning model loads");

    let report = history_all(dir, &[]);

    assert_eq!(report["failed"], json!(0), "{report}");
    assert_eq!(report["vectors"], json!(2), "only the function that moved and the one it left behind changed their history: {report}");
    let read_after = stored_vector(dir, "ler");
    assert_ne!(read_after, read_before, "the vector of the moved function was redone");
    assert!(
        cosine(&earlier, &read_after) > cosine(&earlier, &read_before),
        "the titles of the commits before the move entered the summary: {} against {}",
        cosine(&earlier, &read_after),
        cosine(&earlier, &read_before)
    );
    assert_eq!(stored_vector(dir, "alpha"), alpha_before, "the function whose history was the file's own keeps its vector");

    let again = history_all(dir, &[]);
    assert_eq!(again["vectors"], json!(0), "nothing new, nothing redone: {again}");
    assert_eq!(stored_vector(dir, "ler"), read_after);
}

#[test]
fn the_whole_reading_does_not_read_again_a_file_whose_history_is_still_valid() {
    let temp = project("scan-history-whole-mark-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "cria o beta");
    commit(dir, "src/c.rs", "pub fn gama() {}\n", "cria o gama");
    scan(dir);

    let first = history_all(dir, &[]);
    assert_eq!((first["files"].clone(), first["read"].clone()), (json!(3), json!(3)), "the first reading takes every file and every commit: {first}");
    let before_b = stored_lineage(dir, "src/b.rs").expect("read");
    let nothing = history_all(dir, &[]);
    assert_eq!((nothing["files"].clone(), nothing["read"].clone()), (json!(0), json!(0)), "nothing changed: nothing is read again: {nothing}");

    commit(dir, "src/c.rs", "pub fn gama() { 2 }\n", "muda o gama");
    scan(dir);
    let report = history_all(dir, &[]);
    assert_eq!(report["files"], json!(1), "only the file the new commit touched is read again: {report}");
    let c = stored_lineage(dir, "src/c.rs").expect("read");
    assert_eq!(changes(&c, "gama"), listed(&["muda o gama", "cria o gama"]), "{c:?}");
    assert_eq!(stored_lineage(dir, "src/b.rs").expect("kept"), before_b, "the file that did not change kept its history");

    let moved = history_all(dir, &["--moves", "1"]);
    assert_eq!(
        (moved["files"].clone(), moved["read"].clone()),
        (json!(3), json!(4)),
        "another number of file moves is another reading of every file, from the first commit: {moved}"
    );
}

/// A leitura seguinte parte do que a anterior guardou e pede ao git só os
/// commits que vieram depois dela; o que sai é o mesmo que uma leitura do
/// começo da história daria.
#[test]
fn the_second_reading_reads_only_the_commits_that_came_after_the_first() {
    let temp = project("scan-history-whole-incremental-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "cria o beta");
    commit(dir, "src/c.rs", "pub fn gama(a: u32) -> u32 {\n    a + 1\n}\n", "cria o gama");
    scan(dir);
    let first = history_all(dir, &[]);
    assert_eq!(first["read"], json!(3), "the first reading takes the three commits of the base: {first}");

    write(dir, "src/c.rs", "pub fn gama(a: u32) -> u32 {\n    a + 2\n}\n");
    write(dir, "src/d.rs", "pub fn delta() {}\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "muda o gama e cria o delta"]);
    scan(dir);
    let second = history_all(dir, &[]);
    assert_eq!(second["read"], json!(1), "only the commit that came after the first reading is read: {second}");
    assert_eq!(second["files"], json!(2), "the file it changed and the file it created: {second}");

    let gamma = stored_lineage(dir, "src/c.rs").expect("read");
    assert_eq!(changes(&gamma, "gama"), listed(&["muda o gama e cria o delta", "cria o gama"]), "{gamma:?}");
    let delta = stored_lineage(dir, "src/d.rs").expect("read");
    assert_eq!(changes(&delta, "delta"), listed(&["muda o gama e cria o delta"]), "{delta:?}");

    // A leitura do começo da história dá a mesma lista que a soma das duas.
    let whole = from_the_start(dir, "src/c.rs");
    assert_eq!(gamma, whole, "the list built on top of the first reading is the list the whole history gives");
}

/// Os arquivos lidos em momentos diferentes têm pontas diferentes: cada grupo
/// soma o que veio depois da sua, e nenhum lê o projeto desde o começo.
#[test]
fn files_read_at_different_moments_are_each_brought_up_to_date_from_their_own_point() {
    let temp = project("scan-history-whole-tips-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "cria o beta");
    commit(dir, "src/c.rs", "pub fn gama() {}\n", "cria o gama");
    scan(dir);
    assert_eq!(history_all(dir, &[])["read"], json!(3));

    commit(dir, "src/b.rs", "pub fn beta() { 2 }\n", "muda o beta");
    scan(dir);
    let only_b = history_all(dir, &[]);
    assert_eq!((only_b["files"].clone(), only_b["read"].clone()), (json!(1), json!(1)), "{only_b}");

    write(dir, "src/b.rs", "pub fn beta() { 3 }\n");
    write(dir, "src/c.rs", "pub fn gama() { 3 }\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "muda os dois"]);
    scan(dir);
    let both = history_all(dir, &[]);
    assert_eq!(both["files"], json!(2), "{both}");
    assert_eq!(both["read"], json!(2), "each file reads the one commit that came after its own reading: {both}");
    let b = stored_lineage(dir, "src/b.rs").expect("read");
    assert_eq!(changes(&b, "beta"), listed(&["muda os dois", "muda o beta", "cria o beta"]), "{b:?}");
    let c = stored_lineage(dir, "src/c.rs").expect("read");
    assert_eq!(changes(&c, "gama"), listed(&["muda os dois", "cria o gama"]), "{c:?}");
}

/// A primeira leitura de uma história grande lê só os commits mais novos e
/// diz que parou neles; a que cabe inteira no limite não diz.
#[test]
fn a_first_reading_of_a_long_history_stops_at_the_newest_commits_and_says_so() {
    let temp = project("scan-history-whole-limit-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "cria o beta");
    commit(dir, "src/b.rs", "pub fn beta() { 2 }\n", "muda o beta");
    commit(dir, "src/b.rs", "pub fn beta() { 3 }\n", "muda o beta de novo");
    scan(dir);

    let cut = history_all(dir, &["--newest", "2"]);
    assert_eq!((cut["read"].clone(), cut["limited"].clone()), (json!(2), json!(true)), "{cut}");
    let b = stored_lineage(dir, "src/b.rs").expect("read");
    assert_eq!(changes(&b, "beta"), listed(&["muda o beta de novo", "muda o beta"]), "only the two newest commits are in the list: {b:?}");
    let a = stored_lineage(dir, "src/a.rs").expect("read");
    assert_eq!(
        changes(&a, "alpha"),
        listed(&["muda o beta"]),
        "the function no commit of the window touched is older than all of them: it keeps the oldest commit read: {a:?}"
    );

    let whole = history_all(dir, &["--newest", "4", "--moves", "1"]);
    assert_eq!((whole["read"].clone(), whole["limited"].clone()), (json!(4), json!(false)), "the four commits fit the limit: {whole}");
    let b = stored_lineage(dir, "src/b.rs").expect("read");
    assert_eq!(changes(&b, "beta"), listed(&["muda o beta de novo", "muda o beta", "cria o beta"]), "{b:?}");
}

/// Quando a base foi reescrita, o commit em que a leitura anterior parou não
/// está mais nela: soma-se nada ao que ficou, lê-se a história de novo.
#[test]
fn a_base_rewritten_after_the_first_reading_is_read_from_its_first_commit_again() {
    let temp = project("scan-history-whole-rewrite-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "cria o beta");
    commit(dir, "src/c.rs", "pub fn gama() {}\n", "cria o gama");
    scan(dir);
    assert_eq!(history_all(dir, &[])["read"], json!(3));

    write(dir, "src/c.rs", "pub fn gama() { 2 }\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "--amend", "-m", "cria o gama de outro jeito"]);
    scan(dir);
    let again = history_all(dir, &[]);
    assert_eq!(again["read"], json!(3), "the last commit read is not in the base any more: every commit is read again: {again}");
    let c = stored_lineage(dir, "src/c.rs").expect("read");
    assert_eq!(changes(&c, "gama"), listed(&["cria o gama de outro jeito"]), "{c:?}");
    let b = stored_lineage(dir, "src/b.rs").expect("read");
    assert_eq!(changes(&b, "beta"), listed(&["cria o beta"]), "{b:?}");
}

/// Um arquivo que o mapa passou a ter sem que a leitura anterior o tenha
/// visto, mas que já existia na ponta onde ela parou, tem passado que só a
/// leitura do começo vê.
#[test]
fn a_file_that_existed_before_the_last_reading_and_has_no_history_is_read_from_the_first_commit() {
    let temp = project("scan-history-whole-old-file-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "cria o beta");
    commit(dir, "src/c.rs", "pub fn gama() {}\n", "cria o gama");
    scan(dir);
    assert_eq!(history_all(dir, &[])["read"], json!(3));
    // A história de um arquivo some do mapa (a leitura dele venceu por outra
    // razão que não a base), e outro muda: o mapa quer os dois de volta.
    let model = model::path_in(&dir.join(".claude"));
    let kept = stored_lineage(dir, "src/b.rs").expect("read");
    store::save_lineage_at(&model, &FileLineage { mark: "another scan".into(), ..kept }).expect("the mark of a file is stale");
    commit(dir, "src/c.rs", "pub fn gama() { 2 }\n", "muda o gama");
    scan(dir);
    let again = history_all(dir, &[]);
    assert_eq!(again["files"], json!(2), "{again}");
    assert_eq!(
        again["read"],
        json!(5),
        "the file with no valid list existed in the tip of the last reading, so its history is read from the first commit (4), and the other file from where its own reading stopped (1): {again}"
    );
    let b = stored_lineage(dir, "src/b.rs").expect("read");
    assert_eq!(changes(&b, "beta"), listed(&["cria o beta"]), "{b:?}");
}

/// O arquivo renomeado e mudado no mesmo commit, lido na passada do projeto
/// inteiro: as declarações levam o commit em que nasceram no nome velho.
#[test]
fn a_file_renamed_and_edited_in_one_commit_keeps_the_birth_of_its_declarations_in_the_whole_reading() {
    let temp = project("scan-history-whole-renames-");
    let dir = temp.path();
    declare_base(dir, "main");
    let join_source = |n: u32| {
        format!("pub fn junta(a: u32, b: u32) -> u32 {{\n    let soma = a + b;\n    let dobro = soma * 2;\n    soma + dobro + {n}\n}}\n")
    };
    commit(dir, "src/velho.rs", &join_source(1), "cria o junta");
    git(dir, &["mv", "src/velho.rs", "src/novo.rs"]);
    write(dir, "src/novo.rs", &join_source(2));
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "renomeia e muda o junta"]);
    scan(dir);

    let report = history_all(dir, &[]);
    assert_eq!(report["failed"], json!(0), "{report}");
    let found = stored_lineage(dir, "src/novo.rs").expect("the history of the renamed file was written");
    assert_eq!(changes(&found, "junta"), listed(&["renomeia e muda o junta", "cria o junta"]), "{found:?}");
}

/// A declaração cujas linhas só existem na junção — a que resolve o conflito
/// escrevendo o que nenhum dos lados tinha — é do commit da junção.
#[test]
fn a_declaration_written_only_in_the_merge_gets_the_merge_commit() {
    let temp = project("scan-history-merge-only-");
    let dir = temp.path();
    declare_base(dir, "main");
    let choose_source = |n: u32| format!("pub fn escolhe() -> u32 {{\n    {n}\n}}\n");
    commit(dir, "src/escolha.rs", &choose_source(0), "cria o escolhe");
    git(dir, &["checkout", "-q", "-b", "lado"]);
    commit(dir, "src/escolha.rs", &choose_source(1), "o lado escolhe 1");
    git(dir, &["checkout", "-q", "main"]);
    commit(dir, "src/escolha.rs", &choose_source(2), "a main escolhe 2");
    let merge = Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false", "merge", "-q", "lado"])
        .current_dir(dir)
        .output()
        .expect("run git merge");
    assert!(!merge.status.success(), "the two sides changed the same line: the merge stops on the conflict");
    write(
        dir,
        "src/escolha.rs",
        "pub fn escolhe() -> u32 {\n    3\n}\n\npub fn sum_of_choices(a: u32, b: u32) -> u32 {\n    let total = a + b;\n    total + escolhe()\n}\n",
    );
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "junta os dois lados"]);
    scan(dir);

    let report = history_all(dir, &[]);
    assert_eq!(report["failed"], json!(0), "{report}");
    let found = stored_lineage(dir, "src/escolha.rs").expect("the history of the file was written");
    assert_eq!(changes(&found, "sum_of_choices"), listed(&["junta os dois lados"]), "{found:?}");
    assert_eq!(
        changes(&found, "escolhe"),
        listed(&["junta os dois lados", "cria o escolhe"]),
        "the line the merge wrote is the merge's, and the header of the function is from the commit that created it: {found:?}"
    );
}

/// A linha de fechar um bloco, ou um `else`, se repete pelo projeto todo: se
/// ela contasse, a função que nasce com as mesmas linhas de fechar de outra
/// que sai no mesmo commit herdaria o commit da que saiu.
#[test]
fn a_closing_line_repeated_elsewhere_does_not_give_a_declaration_the_commit_of_another() {
    let temp = project("scan-lineage-trivial-line-");
    let dir = temp.path();
    declare_base(dir, "main");
    let branches = |name: &str, arg: &str, case: u32| {
        format!("pub fn {name}({arg}: u32) -> u32 {{\n    if {arg} > {case} {{\n        {arg} + {case}\n    }} else {{\n        {case}\n    }}\n}}\n")
    };
    commit(dir, "src/ramos.rs", &format!("{}\n{}", branches("zero", "a", 10), branches("dois", "c", 12)), "cria o zero e o dois");
    commit(dir, "src/ramos.rs", &format!("{}\n{}", branches("dois", "c", 12), branches("um", "b", 11)), "troca o zero pelo um");
    scan(dir);

    let found = lineage(dir, "src/ramos.rs");
    assert_eq!(changes(&found, "um"), listed(&["troca o zero pelo um"]), "{found:?}");
    assert_eq!(changes(&found, "dois"), listed(&["cria o zero e o dois"]), "{found:?}");
}

/// Uma busca feita enquanto a leitura roda responde com o que já foi gravado e
/// vê mais a cada lote gravado. Que ela não espera o lote em escrita é
/// provado no núcleo, com a gravação aberta; aqui basta que responda. Os
/// arquivos contados são os que as palavras da busca acham: a lista de
/// candidatos leva também as declarações que o sentido do pedido põe perto,
/// mesmo quando nenhuma palavra casa, e por isso não conta a leitura.
#[test]
fn a_search_made_while_the_history_is_being_read_answers_with_what_is_stored() {
    let temp = project("scan-history-whole-search-");
    let dir = temp.path();
    let total = 60;
    for at in 0..total {
        write(dir, &format!("src/f{at}.rs"), &format!("pub fn f{at}() {{}}\n"));
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "ajusta as funções do guardanapo"]);
    scan(dir);
    let model = model::path_in(&dir.join(".claude"));
    let languages = Languages::of(&ProjectConfig::default());
    let found = |asked: &str| {
        candidates_at(&model, asked, "", &languages, any_path).expect("the candidates answer");
        triage_at(&model, (asked, ""), &languages, 100).expect("the search answers").files.len()
    };
    assert_eq!(found("guardanapo"), 0, "the word is only in the commit title, which is not read yet");

    let mut reading = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["history-all", dir.to_str().unwrap(), "--out", model.to_str().unwrap(), "--batch", "1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("run scan history-all");
    let mut seen: Vec<usize> = Vec::new();
    loop {
        seen.push(found("guardanapo"));
        if reading.try_wait().expect("wait").is_some() {
            break;
        }
    }
    assert!(reading.wait().expect("wait").success(), "the reading ends well");
    assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]), "the search only sees more as the batches are written: {seen:?}");
    assert_eq!(found("guardanapo"), total, "after the reading every function is found by the word of its commit: {seen:?}");
}

/// A história de um arquivo lê os mesmos `--newest` commits da leitura do
/// projeto inteiro: ela lê só os mais novos e diz que parou neles; o limite
/// que cabe a história toda não diz.
#[test]
fn the_history_of_one_file_reads_only_the_newest_commits_and_says_so() {
    let temp = project("scan-history-file-limit-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "cria o beta");
    commit(dir, "src/b.rs", "pub fn beta() { 2 }\n", "muda o beta");
    commit(dir, "src/b.rs", "pub fn beta() { 3 }\n", "muda o beta de novo");
    scan(dir);

    let (cut, b) = history_in(dir, &dir.join(".claude"), "src/b.rs", &["--newest", "2"]);
    assert_eq!((cut["read"].clone(), cut["limited"].clone()), (json!(2), json!(true)), "{cut}");
    assert_eq!(changes(&b, "beta"), listed(&["muda o beta de novo", "muda o beta"]), "only the two newest commits are in the list: {b:?}");

    let other = tempfile::tempdir().unwrap();
    model::scan(dir, other.path(), &[]);
    let (whole, b) = history_in(dir, other.path(), "src/b.rs", &["--newest", "4"]);
    assert_eq!((whole["read"].clone(), whole["limited"].clone()), (json!(4), json!(false)), "the four commits fit the limit: {whole}");
    assert_eq!(changes(&b, "beta"), listed(&["muda o beta de novo", "muda o beta", "cria o beta"]), "{b:?}");

    let (usual, _) = history_in(dir, other.path(), "src/a.rs", &[]);
    assert_eq!((usual["read"].clone(), usual["limited"].clone()), (json!(4), json!(false)), "the default limit is the one of the whole reading: {usual}");
}

/// O arquivo que já tem a lista guardada numa ponta antiga parte dela: a
/// passada de um arquivo lê só os commits que vieram depois da ponta, e dá a
/// mesma lista que a leitura do começo.
#[test]
fn a_file_with_a_stored_history_reads_only_the_commits_after_the_tip_it_was_read_at() {
    let temp = project("scan-history-file-tip-");
    let dir = temp.path();
    commit(dir, "src/b.rs", "pub fn beta() {}\n", "cria o beta");
    scan(dir);
    assert_eq!(history_all(dir, &[])["read"], json!(2), "the two commits of the base");

    commit(dir, "src/b.rs", "pub fn beta() { 2 }\n", "muda o beta");
    scan(dir);
    let (report, b) = history_in(dir, &dir.join(".claude"), "src/b.rs", &[]);
    assert_eq!((report["read"].clone(), report["limited"].clone()), (json!(1), json!(false)), "only the commit after the tip is read: {report}");
    assert_eq!(changes(&b, "beta"), listed(&["muda o beta", "cria o beta"]), "{b:?}");
    assert_eq!(b, from_the_start(dir, "src/b.rs"), "the list built on the stored one is the list of the whole history");
}

const F_SHORT: &str = "pub fn f(a: u32) -> u32 {\n    let b = a + 1;\n    b\n}\n";
const F_LONG: &str = "pub fn f(a: u32) -> u32 {\n    let b = a + 1;\n    let c = b * 2;\n    b\n}\n";

/// Um projeto com uma função que ganhou uma linha, lido do começo da história
/// (o mapa guarda a lista de cada arquivo). A leitura curta que vem depois de a
/// linha sair guarda a função com o commit da linha que já não existe.
fn read_project_with_a_line_to_lose(prefix: &str) -> tempfile::TempDir {
    let temp = project(prefix);
    let dir = temp.path();
    declare_base(dir, "main");
    commit(dir, "src/f.rs", F_SHORT, "cria o f");
    commit(dir, "src/f.rs", F_LONG, "adiciona a linha");
    scan(dir);
    assert_eq!(history_all(dir, &[])["read"], json!(3), "the whole reading takes the three commits");
    assert_eq!(changes(&stored_lineage(dir, "src/f.rs").expect("read"), "f"), listed(&["adiciona a linha", "cria o f"]));
    temp
}

/// Uma rodada de leitura curta: `commits` commits em `src/noise.rs`, o scan e a
/// leitura do projeto inteiro; devolve o relato dela.
fn a_short_round(dir: &Path, commits: u32) -> Value {
    for _ in 0..commits {
        let at = git(dir, &["rev-list", "--count", "HEAD"]).trim().to_string();
        commit(dir, "src/noise.rs", &format!("pub fn noise() {{ {at} }}\n"), &format!("ruído {at}"));
    }
    scan(dir);
    history_all(dir, &[])
}

/// A lista gravada de cada arquivo do mapa da pasta `out`, em ordem de caminho.
fn stored_lineages(out: &Path) -> Vec<FileLineage> {
    let mut all = store::read_at(&model::path_in(out)).expect("o mapa se lê").lineage;
    all.sort_by(|a, b| a.path.cmp(&b.path));
    all
}

/// Passadas curtas que somam 50 commits deixam a lista aproximada, e a leitura
/// seguinte volta ao começo da história com todos os arquivos: a lista fica
/// igual, declaração a declaração, à de uma leitura feita do zero, e a soma
/// volta a zero.
#[test]
fn fifty_commits_of_short_readings_bring_the_next_reading_back_to_the_start_and_the_history_becomes_exact() {
    let temp = read_project_with_a_line_to_lose("scan-history-rereading-");
    let dir = temp.path();
    let out = dir.join(".claude");
    commit(dir, "src/f.rs", F_SHORT, "remove a linha");
    for (round, commits) in [9, 10, 10, 10, 10].into_iter().enumerate() {
        let report = a_short_round(dir, commits);
        assert_eq!(report["read"], json!(10), "round {round}: only the commits after the last reading are read: {report}");
    }
    assert_eq!(
        changes(&stored_lineage(dir, "src/f.rs").expect("read"), "f"),
        listed(&["adiciona a linha", "cria o f"]),
        "the short readings only add: the commit of the line that left is still in the list"
    );

    let total = git(dir, &["rev-list", "--count", "HEAD"]).trim().parse::<u64>().unwrap() + 1;
    let whole = a_short_round(dir, 1);
    assert_eq!(whole["read"], json!(total), "the 50 commits are added up: this reading is from the first commit: {whole}");
    assert_eq!((whole["files"].clone(), whole["limited"].clone()), (json!(3), json!(false)), "every file of the map is read again: {whole}");
    assert_eq!(changes(&stored_lineage(dir, "src/f.rs").expect("read"), "f"), listed(&["cria o f"]), "the line that left takes its commit along");

    let fresh = tempfile::tempdir().unwrap();
    model::scan(dir, fresh.path(), &[]);
    history_all_in(dir, fresh.path(), &[]);
    assert_eq!(stored_lineages(&out), stored_lineages(fresh.path()), "the list of every file is the one a reading from zero gives");

    let short_again = a_short_round(dir, 1);
    assert_eq!(short_again["read"], json!(1), "the sum went back to zero: the next reading is short: {short_again}");
}

/// O arquivo que saiu do mapa deixa a lista dele guardada, e nenhuma leitura
/// curta a revê. A leitura inteira que vem aos 50 commits a leva embora, e a
/// lista de todos os arquivos fica a de uma leitura feita do zero.
#[test]
fn the_whole_reading_drops_the_history_of_a_file_that_left_the_map_and_the_short_ones_keep_it() {
    let temp = read_project_with_a_line_to_lose("scan-history-left-the-map-");
    let dir = temp.path();
    let out = dir.join(".claude");
    commit(dir, "src/gone.rs", "pub fn gone() {}\n", "cria o gone");
    scan(dir);
    history_all(dir, &[]);
    let has_gone = || stored_lineages(&out).iter().any(|lineage| lineage.path == "src/gone.rs");
    assert!(has_gone(), "the file is in the map and has its history");
    git(dir, &["rm", "-q", "src/gone.rs"]);
    git(dir, &["commit", "-q", "-m", "remove o gone"]);

    let mut short_rounds = 0;
    loop {
        let report = a_short_round(dir, 10);
        if report["files"] == json!(3) {
            break;
        }
        assert!(has_gone(), "a short reading does not go back to a file that left the map: round {short_rounds}: {report}");
        short_rounds += 1;
        assert!(short_rounds < 10, "the whole reading never came");
    }
    assert!(short_rounds >= 5, "the whole reading came only after the short ones added up: {short_rounds} rounds");
    assert!(!has_gone(), "the whole reading took the history of the file that left the map");

    let fresh = tempfile::tempdir().unwrap();
    model::scan(dir, fresh.path(), &[]);
    history_all_in(dir, fresh.path(), &[]);
    assert_eq!(stored_lineages(&out), stored_lineages(fresh.path()), "the list of every file is the one a reading from zero gives");
}

/// Abaixo de 50 commits somados, a leitura seguinte segue curta: só lê os
/// commits novos e só os arquivos que mudaram.
#[test]
fn below_fifty_commits_of_short_readings_the_next_one_stays_short_and_reads_only_the_new_commits() {
    let temp = read_project_with_a_line_to_lose("scan-history-rereading-below-");
    let dir = temp.path();
    commit(dir, "src/f.rs", F_SHORT, "remove a linha");
    for (round, commits) in [8, 10, 10, 10, 10].into_iter().enumerate() {
        let report = a_short_round(dir, commits);
        let read = if round == 0 { commits + 1 } else { commits };
        assert_eq!(report["read"], json!(read), "round {round}: {report}");
    }

    let next = a_short_round(dir, 1);
    assert_eq!(next["read"], json!(1), "49 commits are not enough: only the new commit is read: {next}");
    assert_eq!(next["files"], json!(1), "only the file the new commit changed: {next}");
    assert_eq!(
        changes(&stored_lineage(dir, "src/f.rs").expect("read"), "f"),
        listed(&["adiciona a linha", "cria o f"]),
        "the file that did not change was not read again"
    );
}

/// A leitura em segundo plano baixa a prioridade do próprio processo, e o git
/// que ela abre herda: o `git` posto na frente no `PATH` anota a prioridade
/// que recebeu.
#[cfg(unix)]
#[test]
fn the_reading_in_the_background_runs_at_a_lower_priority_and_so_does_the_git_it_opens() {
    use std::os::unix::fs::PermissionsExt;

    let niceness = |pid: &str| -> i32 {
        let out = Command::new("ps").args(["-o", "ni=", "-p", pid]).output().expect("run ps");
        String::from_utf8_lossy(&out.stdout).trim().parse().expect("ps says the priority")
    };
    let temp = project("scan-history-priority-");
    let dir = temp.path();
    scan(dir);

    let shim = tempfile::tempdir().unwrap();
    let log = shim.path().join("niceness.log");
    let script = shim.path().join("git");
    std::fs::write(&script, "#!/bin/sh\nps -o ni= -p $$ | tr -d ' ' >> \"$NICENESS_LOG\"\nPATH=\"${PATH#*:}\"\nexec git \"$@\"\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", shim.path().display(), std::env::var("PATH").unwrap());

    let model = model::path_in(&dir.join(".claude"));
    let run = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["history-all", dir.to_str().unwrap(), "--out", model.to_str().unwrap(), "--json"])
        .env("PATH", path)
        .env("NICENESS_LOG", &log)
        .output()
        .expect("run scan history-all");
    assert!(run.status.success(), "stderr: {}", String::from_utf8_lossy(&run.stderr));

    // A prioridade só se baixa: quem já roda com mais de 10 fica como está.
    let expected = niceness(&std::process::id().to_string()).max(10);
    let logged: Vec<i32> = std::fs::read_to_string(&log).expect("the git of the shim ran").lines().map(|line| line.parse().unwrap()).collect();
    assert!(!logged.is_empty(), "the reading opened git");
    assert!(logged.iter().all(|priority| *priority == expected), "every git the reading opened has the priority {expected}: {logged:?}");
}
