//! O scan grava o mapa no banco SQLite: a primeira passada cria o banco e
//! apaga o mapa em JSON de antes dele, e a passada que não acha nada mudado
//! não regrava o arquivo — os mesmos bytes e a mesma data, também quando lê
//! tudo de novo.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::time::SystemTime;

use mustard_core::io::project_map as store;
use serde_json::Value;

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// Os bytes e a data do banco da pasta `folder`.
fn on_disk(folder: &Path) -> (Vec<u8>, SystemTime) {
    let db = model::path_in(folder);
    (std::fs::read(&db).unwrap(), std::fs::metadata(&db).unwrap().modified().unwrap())
}

/// Um projeto pequeno, com o mapa em JSON de antes do banco na pasta onde o
/// banco vai morar.
fn small_project(dir: &Path) {
    write(dir, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write(dir, "src/lib.rs", "pub mod a;\n");
    write(dir, "src/a.rs", "/// Soma um.\npub fn alpha(x: u32) -> u32 {\n    x + 1\n}\n");
    write(dir, &format!(".claude/{}", store::LEGACY_MAP_FILE_NAME), "{\"modules\":[]}\n");
}

#[test]
fn the_scan_writes_the_database_and_deletes_the_json_map_of_before() {
    let temp = tempfile::Builder::new().prefix("scan-map-db-").tempdir().unwrap();
    let dir = temp.path();
    small_project(dir);
    let folder = dir.join(".claude");

    let (map, report) = model::scan(dir, &folder, &[]);
    assert_eq!(report["full"], Value::Bool(true), "{report}");

    let db = std::fs::read(model::path_in(&folder)).unwrap();
    assert!(db.starts_with(b"SQLite format 3\0"), "the map is a SQLite database");
    assert!(!folder.join(store::LEGACY_MAP_FILE_NAME).exists(), "the JSON map of before is deleted");
    for beside in [store::MAP_JOURNAL_FILE_NAME, store::MAP_WAL_FILE_NAME, store::MAP_SHARED_FILE_NAME] {
        assert!(!folder.join(beside).exists(), "{beside} is not left behind");
    }

    let alpha = map["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|module| module["path"] == "src/a.rs")
        .and_then(|module| module["declarations"].as_array())
        .and_then(|decls| decls.iter().find(|decl| decl["name"] == "alpha"))
        .cloned()
        .expect("the declaration of src/a.rs is in the map");
    assert_eq!(alpha["doc"], "Soma um.");
    assert_eq!(alpha["line"], 2);
    assert_eq!(map["languages"][0]["language"], "rust", "{}", map["languages"]);
}

#[test]
fn a_pass_that_finds_nothing_changed_rewrites_nothing() {
    let temp = tempfile::Builder::new().prefix("scan-map-db-same-").tempdir().unwrap();
    let dir = temp.path();
    small_project(dir);
    let folder = dir.join(".claude");
    model::scan(dir, &folder, &[]);
    let before = on_disk(&folder);

    // A data do arquivo tem de poder mudar entre as passadas.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    for extra in [&[][..], &["--all"][..]] {
        model::scan(dir, &folder, extra);
        let after = on_disk(&folder);
        assert!(after.0 == before.0, "{extra:?}: the bytes of the map changed");
        assert_eq!(after.1, before.1, "{extra:?}: the map was written again");
    }

    // O que muda é gravado.
    write(dir, "src/a.rs", "/// Soma dois.\npub fn alpha(x: u32) -> u32 {\n    x + 2\n}\n");
    model::scan(dir, &folder, &[]);
    assert!(on_disk(&folder).0 != before.0, "a changed file is written");
}

/// As colunas da tabela `table` no banco da pasta `folder`, em ordem.
fn columns(folder: &Path, table: &str) -> Vec<String> {
    let conn = rusqlite::Connection::open(model::path_in(folder)).unwrap();
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid").unwrap();
    stmt.query_map([table], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
}

/// As dependências moram só nos manifestos: o banco recém-gravado pelo scan
/// não tem a coluna delas na tabela dos projetos, e a tabela dos manifestos
/// segue com ela, cheia.
#[test]
fn the_dependencies_live_in_the_manifests_and_not_in_the_projects() {
    let temp = tempfile::Builder::new().prefix("scan-map-db-deps-").tempdir().unwrap();
    let dir = temp.path();
    small_project(dir);
    write(dir, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\n");
    let folder = dir.join(".claude");
    let (map, _) = model::scan(dir, &folder, &[]);

    let projects = columns(&folder, "projects");
    assert!(projects.contains(&"frameworks".to_string()), "{projects:?}");
    assert!(!projects.contains(&"dependencies".to_string()), "{projects:?}");
    let manifests = columns(&folder, "manifests");
    assert!(manifests.contains(&"dependencies".to_string()), "{manifests:?}");
    assert_eq!(map["manifests"][0]["dependencies"], serde_json::json!(["serde"]), "{}", map["manifests"]);
    assert_eq!(map["projects"][0]["frameworks"], serde_json::json!(["serde"]), "{}", map["projects"]);
}

/// O vetor da declaração `name` no banco da pasta `folder`, e quantos vetores
/// de declaração e de palavra o banco tem.
fn vector_of(folder: &Path, name: &str) -> (Option<Vec<u8>>, i64, i64) {
    let conn = rusqlite::Connection::open(model::path_in(folder)).unwrap();
    let count = |table: &str| -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
    };
    let vector = conn
        .query_row("SELECT vector FROM decl_vectors WHERE name = ?1", [name], |row| row.get::<_, Vec<u8>>(0))
        .ok();
    (vector, count("decl_vectors"), count("word_vectors"))
}

/// A passada do scan grava o vetor de cada declaração e de cada palavra do
/// projeto no mapa: 256 bytes por declaração, e a que muda ganha o vetor
/// novo, sem que a passada seguinte, sem mudança, regrave o arquivo.
#[test]
fn the_scan_pass_writes_a_vector_for_each_declaration_and_each_word() {
    let temp = tempfile::Builder::new().prefix("scan-map-vectors-").tempdir().unwrap();
    let dir = temp.path();
    small_project(dir);
    write(dir, "mustard.json", r#"{"ai":{"vectors":true}}"#);
    let folder = dir.join(".claude");
    model::scan(dir, &folder, &[]);

    let (alpha, declarations, words) = vector_of(&folder, "alpha");
    assert_eq!(alpha.as_ref().map(Vec::len), Some(256), "one int8 number per dimension");
    assert_eq!(declarations, 1);
    assert!(words > 0, "the words of the project have vectors");

    std::thread::sleep(std::time::Duration::from_millis(1100));
    let before = on_disk(&folder);
    model::scan(dir, &folder, &[]);
    let after = on_disk(&folder);
    assert!(after.0 == before.0 && after.1 == before.1, "a pass without change writes nothing");

    write(dir, "src/a.rs", "/// Apaga o arquivo do disco.\npub fn alpha(x: u32) -> u32 {\n    x + 1\n}\n");
    model::scan(dir, &folder, &[]);
    let (changed, declarations, _) = vector_of(&folder, "alpha");
    assert_eq!(declarations, 1);
    assert!(changed != alpha, "the changed declaration gets a new vector");
}
