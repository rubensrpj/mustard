//! A prova de versão das medidas confere a marca de cada bloco do mapa com a
//! que o `scan` compilado diz em `scan format`. Este teste liga as duas pontas
//! de verdade: o mapa que o próprio scan grava passa na conferência, em todos
//! os blocos, e o mapa gravado com a marca de outra compilação é recusado
//! dizendo as duas marcas. A árvore cujo mapa é de outra compilação é refeita
//! pelo mesmo scan e passa a ser aceita, e o mapa refeito volta com a história
//! de cada declaração já lida do git.

#[path = "support/model.rs"]
mod model;

use std::path::Path;

use mustard_core::domain::config::ProjectConfig;
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::project_map::FileLineage;
use mustard_core::domain::scan::Scan;
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::map_lineage::reading_lock_path;
use mustard_core::io::measure_proof::{check_map, rebuild_map, rebuild_map_telling};
use mustard_core::io::project_map as store;

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// Um projeto pequeno, mapeado pelo scan compilado com o teste na pasta `.claude`.
fn mapped_project(dir: &Path) -> std::path::PathBuf {
    write(dir, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write(dir, "src/lib.rs", "pub mod a;\n");
    write(dir, "src/a.rs", "/// Soma um.\npub fn alpha(x: u32) -> u32 {\n    x + 1\n}\n");
    let folder = dir.join(".claude");
    model::scan(dir, &folder, &[]);
    model::path_in(&folder)
}

#[test]
fn a_map_written_by_this_scan_passes_the_check_with_the_mark_it_says() {
    let temp = tempfile::Builder::new().prefix("scan-measure-proof-").tempdir().unwrap();
    let map = mapped_project(temp.path());
    let expected = model::scan_format();

    let proof = check_map(&map, &expected).unwrap_or_else(|refusal| panic!("o mapa do próprio scan é recusado: {refusal}"));

    assert_eq!(proof.mark, expected);
    assert_eq!(proof.path, map.display().to_string());
    // Todos os blocos do mapa trazem a marca do scan, não só um deles.
    let marks = store::read_marks_at(&map).unwrap();
    assert_eq!(marks.len(), store::BLOCKS.len(), "{marks:?}");
    assert!(marks.values().all(|mark| *mark == expected), "{marks:?}");
}

#[test]
fn a_map_written_with_the_mark_of_another_scan_is_refused_with_both_marks() {
    let temp = tempfile::Builder::new().prefix("scan-measure-proof-").tempdir().unwrap();
    let map = mapped_project(temp.path());
    let expected = model::scan_format();

    // O mesmo conteúdo, regravado como se outra compilação do scan o tivesse feito.
    let stored = store::read_stored_at(&map).unwrap();
    let value: serde_json::Value = serde_json::from_str(&stored.json).unwrap();
    let other = format!("{expected}-de-outra-compilacao");
    store::save_at(&map, &value, &other, &Languages::of(&ProjectConfig::default())).unwrap();

    let refusal = check_map(&map, &expected).unwrap_err().to_string();
    assert!(refusal.contains(&other) && refusal.contains(&expected), "{refusal}");
}

/// O mapa que ficou na árvore de uma compilação velha do scan é recusado pela
/// conferência; refeito pelo scan compilado com o teste, a mesma árvore passa,
/// com a marca dele em todos os blocos e nada do mapa velho.
#[test]
fn a_tree_with_the_map_of_another_scan_is_rebuilt_and_then_passes_the_check() {
    let temp = tempfile::Builder::new().prefix("scan-measure-proof-").tempdir().unwrap();
    let map = mapped_project(temp.path());
    let expected = model::scan_format();
    let old = "scan de outra compilacao";
    model::mark_as(&temp.path().join(".claude"), old);
    // Um arquivo que o SQLite deixa ao lado do banco velho também sai.
    let beside = map.with_file_name(store::MAP_WAL_FILE_NAME);
    std::fs::write(&beside, "de outra compilação").unwrap();
    let refusal = check_map(&map, &expected).unwrap_err().to_string();
    assert!(refusal.contains(old), "o mapa velho é recusado: {refusal}");

    let rebuilt = rebuild_map(temp.path(), &Scan::new(env!("CARGO_BIN_EXE_scan"))).unwrap_or_else(|e| panic!("o scan não refez a árvore: {e}"));

    assert!(rebuilt.scan.full, "o mapa foi lido do zero: {rebuilt:?}");
    let proof = check_map(&map, &expected).unwrap_or_else(|refusal| panic!("a árvore refeita é recusada: {refusal}"));
    assert_eq!(proof.mark, expected);
    let marks = store::read_marks_at(&map).unwrap();
    assert!(marks.values().all(|mark| *mark == expected), "nada da marca velha ficou: {marks:?}");
}

/// Um commit em `dir`, com a identidade do teste.
fn git(dir: &Path, args: &[&str]) {
    let run = std::process::Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(run.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&run.stderr));
}

/// A história que o mapa em `map` guarda para o arquivo `file`, quando já a tem.
fn lineage_of(map: &Path, file: &str) -> Option<FileLineage> {
    store::read_at(map).ok()?.lineage.into_iter().find(|lineage| lineage.path == file)
}

/// Um repositório com dois commits sobre a função `alpha` de `src/a.rs`.
fn repo_with_two_commits(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    write(dir, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write(dir, "src/lib.rs", "pub mod a;\n");
    write(dir, "src/a.rs", "pub fn alpha(x: u32) -> u32 {\n    x + 1\n}\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "cria o alpha"]);
    write(dir, "src/a.rs", "pub fn alpha(x: u32) -> u32 {\n    x + 2\n}\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "muda o alpha"]);
}

/// Quantas linhas de `lineage_decls` o banco em `map` tem.
fn lineage_rows(map: &Path) -> i64 {
    let conn = rusqlite::Connection::open(map).unwrap();
    conn.query_row("SELECT COUNT(*) FROM lineage_decls", [], |row| row.get(0)).unwrap()
}

/// O mapa refeito para a medida tem o scan e, depois dele, a leitura da
/// história de cada declaração, esperada até o fim: num repositório com dois
/// commits sobre uma função, a conta de `lineage_decls` feita logo na volta de
/// `rebuild_map`, sem esperar, já traz as linhas, com os títulos dos dois
/// commits, e a linha de peças da prova diz que o histórico está ligado. O
/// scan sozinho deixaria `lineage_*` vazio.
#[test]
fn a_map_rebuilt_for_a_measure_has_the_history_of_each_declaration_the_moment_the_call_returns() {
    let temp = tempfile::Builder::new().prefix("scan-measure-history-").tempdir().unwrap();
    let dir = temp.path();
    repo_with_two_commits(dir);
    let map = model::path_in(&dir.join(".claude"));

    rebuild_map(dir, &Scan::new(env!("CARGO_BIN_EXE_scan"))).unwrap_or_else(|e| panic!("o scan não refez a árvore: {e}"));

    assert!(lineage_rows(&map) > 0, "o banco refeito volta com a história de cada declaração, sem esperar por ela");
    let lineage = lineage_of(&map, "src/a.rs").expect("a história do arquivo chegou antes da volta");
    let alpha = lineage.declarations.iter().find(|decl| decl.name == "alpha").expect("a história do alpha chegou");
    let titles: Vec<String> =
        alpha.commits.iter().filter_map(|change| lineage.commits.iter().find(|commit| commit.id == change.id)).map(|commit| commit.title.clone()).collect();
    assert_eq!(titles, ["muda o alpha", "cria o alpha"]);

    let proof = check_map(&map, &model::scan_format()).unwrap_or_else(|refusal| panic!("o mapa refeito é recusado: {refusal}"));
    let line = proof.pieces_line();
    assert!(line.contains("historico=ligada"), "a linha de peças diz que o histórico chegou: {line}");
}

/// Uma sessão aberta no projeto já está lendo a história do mapa (quem lê
/// segura a trava da leitura) quando a medida chega, e solta um segundo depois
/// de a medida dizer que está lendo a história: a medida espera a outra
/// leitura acabar em vez de recusar, e volta, só depois da soltura, com a
/// história de cada declaração, sem arquivo que o git não leu.
#[test]
fn a_map_rebuilt_while_another_reading_holds_the_history_waits_for_it_and_comes_back_with_the_history() {
    let temp = tempfile::Builder::new().prefix("scan-measure-waits-").tempdir().unwrap();
    let dir = temp.path();
    repo_with_two_commits(dir);
    let map = model::path_in(&dir.join(".claude"));
    let other_reading = LockedFile::exclusive(&reading_lock_path(&map)).unwrap();
    let (said, hears) = std::sync::mpsc::channel::<()>();
    let releasing = std::thread::spawn(move || {
        // A trava só solta um segundo depois de a medida chegar à leitura da
        // história: ela está presa quando o scan é pedido, por mais lenta que
        // seja a máquina.
        hears.recv().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let released_at = std::time::Instant::now();
        drop(other_reading);
        released_at
    });

    let rebuilt = rebuild_map_telling(dir, &Scan::new(env!("CARGO_BIN_EXE_scan")), &|_| {
        let _ = said.send(());
    })
    .unwrap_or_else(|e| panic!("a medida recusou em vez de esperar: {e}"));
    let back_at = std::time::Instant::now();
    let released_at = releasing.join().unwrap();

    assert!(back_at >= released_at, "a medida voltou antes de a outra leitura soltar a trava");
    assert_eq!(rebuilt.unread, 0, "{rebuilt:?}");
    assert!(lineage_rows(&map) > 0, "o mapa refeito volta com a história de cada declaração");
}
