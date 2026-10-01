//! A prova de versão das medidas confere a marca de cada bloco do mapa com a
//! que o `scan` compilado diz em `scan format`. Este teste liga as duas pontas
//! de verdade: o mapa que o próprio scan grava passa na conferência, em todos
//! os blocos, e o mapa gravado com a marca de outra compilação é recusado
//! dizendo as duas marcas.

#[path = "support/model.rs"]
mod model;

use std::path::Path;

use mustard_core::domain::config::ProjectConfig;
use mustard_core::domain::normalize::Languages;
use mustard_core::io::measure_proof::check_map;
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
