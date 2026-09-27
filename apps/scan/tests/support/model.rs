//! O mapa que o scan grava, visto pelos testes do pacote num lugar só: onde
//! ele mora numa pasta, a passada do scan que o grava, a leitura dele e a
//! gravação de um mapa escrito à mão. Quando o formato do mapa mudar, só este
//! arquivo muda com ele.
//!
//! O mapa é um banco SQLite; a leitura daqui o devolve no JSON do scan, pela
//! porta do núcleo, que é o que os testes conferem.
//!
//! Um arquivo por pacote: os testes da pasta `tests/` o trazem pelo caminho,
//! como o `manifest_dir.rs`. Cada teste usa só uma parte dele.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_core::io::project_map as store;
use serde_json::Value;

/// O caminho do mapa gravado na pasta `dir`.
pub fn path_in(dir: &Path) -> PathBuf {
    dir.join(store::MAP_FILE_NAME)
}

/// Roda o scan sobre `root`, gravando o mapa na pasta `out`, com `extra`
/// depois dos argumentos de sempre, e devolve o mapa gravado e o relato da
/// passada, a linha JSON que ela imprime.
pub fn scan(root: &Path, out: &Path, extra: &[&str]) -> (Value, Value) {
    let model = path_in(out);
    let run = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", root.to_str().unwrap(), "--out", model.to_str().unwrap(), "--json"])
        .args(extra)
        .output()
        .expect("run scan");
    assert!(run.status.success(), "stderr: {}", String::from_utf8_lossy(&run.stderr));
    let stdout = String::from_utf8_lossy(&run.stdout);
    let report = serde_json::from_str(stdout.lines().last().unwrap_or("{}")).expect("o relato é uma linha JSON");
    (read(out), report)
}

/// O mapa já gravado na pasta `dir`, no JSON do scan.
pub fn read(dir: &Path) -> Value {
    serde_json::from_slice(&read_bytes(dir)).expect("o mapa é JSON")
}

/// O mapa já gravado na pasta `dir`, como texto JSON: dois mapas com o mesmo
/// conteúdo dão o mesmo texto.
pub fn read_bytes(dir: &Path) -> Vec<u8> {
    store::read_stored_at(&path_in(dir)).expect("o mapa foi gravado e se lê").json.into_bytes()
}

/// Grava `text` como o mapa da pasta `dir` e devolve o caminho dele: um mapa
/// escrito à mão, para o comando que o lê.
pub fn write(dir: &Path, text: &str) -> PathBuf {
    let model = path_in(dir);
    store::write_text_at(&model, text).expect("grava o mapa");
    model
}
