//! O que o mapa passa a guardar de cada declaração: o comentário de
//! documentação escrito em cima dela, a assinatura dela, e cada ligação
//! nomeada entre declarações — quem chama quem, em qual arquivo e em qual
//! linha. A varredura é a de verdade, num projeto de mentira gravado em disco,
//! e a conferência é feita no mapa gravado.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn model_of(dir: &Path) -> PathBuf {
    dir.join(".claude").join("grain.model.json")
}

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// Varre `dir` e devolve o mapa gravado.
fn scan(dir: &Path) -> Value {
    let model = model_of(dir);
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", dir.to_str().unwrap(), "--out", model.to_str().unwrap(), "--json"])
        .output()
        .expect("run scan");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&model).expect("the map was written");
    serde_json::from_str(&text).expect("the map is JSON")
}

/// Um projeto de mentira: uma função documentada em português, e outro arquivo
/// que a chama duas vezes de dentro de uma função sua.
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scan-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write(&dir, "Cargo.toml", "[package]\nname = \"loja\"\nversion = \"0.1.0\"\n");
    write(&dir, "src/lib.rs", "pub mod preco;\npub mod pedido;\n");
    write(
        &dir,
        "src/preco.rs",
        "/// Soma o preço do pedido com o frete.\n\
         ///\n\
         /// O frete vem em centavos.\n\
         pub fn total(preco: u32, frete: u32) -> u32 {\n    \
             preco + frete\n\
         }\n\n\
         pub fn sem_documento() -> u32 {\n    \
             0\n\
         }\n",
    );
    write(
        &dir,
        "src/pedido.rs",
        "use crate::preco::total;\n\n\
         /// Fecha o pedido.\n\
         pub fn fechar() -> u32 {\n    \
             let parcial = total(10, 2);\n    \
             total(parcial, 1)\n\
         }\n",
    );
    dir
}

/// A declaração `name` do arquivo `file`, como o mapa a gravou.
fn declaration<'a>(map: &'a Value, file: &str, name: &str) -> &'a Value {
    let module = map["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["path"] == file)
        .unwrap_or_else(|| panic!("{file} não está no mapa: {map}"));
    module["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == name)
        .unwrap_or_else(|| panic!("{name} não está em {file}: {module}"))
}

#[test]
fn o_mapa_guarda_o_comentario_e_a_assinatura_de_cada_declaracao() {
    let dir = project("doc-e-assinatura");
    let map = scan(&dir);

    let total = declaration(&map, "src/preco.rs", "total");
    assert_eq!(
        total["doc"], "Soma o preço do pedido com o frete. O frete vem em centavos.",
        "o comentário de documentação inteiro, sem as barras: {total}"
    );
    assert_eq!(
        total["signature"], "pub fn total(preco: u32, frete: u32) -> u32",
        "a assinatura, sem o corpo: {total}"
    );
    // O que já se guardava continua lá.
    assert_eq!(total["kind"], "function", "{total}");
    assert_eq!(total["line"], 4, "{total}");
    assert_eq!(total["end_line"], 6, "{total}");

    // Uma declaração sem comentário em cima traz o campo vazio, e ainda assim
    // a assinatura.
    let sem = declaration(&map, "src/preco.rs", "sem_documento");
    assert_eq!(sem.get("doc").map_or("", |d| d.as_str().unwrap_or("")), "", "{sem}");
    assert_eq!(sem["signature"], "pub fn sem_documento() -> u32", "{sem}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn o_mapa_guarda_cada_uso_de_cada_declaracao() {
    let dir = project("usos");
    let map = scan(&dir);

    // Quem é usado: as duas chamadas, cada uma com o arquivo, a linha e a
    // declaração de onde parte.
    let total = declaration(&map, "src/preco.rs", "total");
    let used_by: Vec<&str> = total["used_by"].as_array().unwrap().iter().map(|u| u.as_str().unwrap()).collect();
    assert_eq!(used_by, vec!["src/pedido.rs:5:fechar", "src/pedido.rs:6:fechar"], "cada uso, não a contagem: {total}");

    // Quem chama: o outro lado da mesma ligação.
    let fechar = declaration(&map, "src/pedido.rs", "fechar");
    assert_eq!(fechar["calls"], serde_json::json!(["total"]), "{fechar}");

    // Uma declaração que ninguém usa não ganha ligação nenhuma.
    let sem = declaration(&map, "src/preco.rs", "sem_documento");
    assert!(sem.get("used_by").is_none(), "{sem}");
    assert!(sem.get("calls").is_none(), "{sem}");

    // As contagens do grafo continuam onde estavam — a ligação nomeada é nova,
    // e não substitui o que o grafo já dizia dos arquivos.
    assert!(map["graph"]["nodes"].as_u64().unwrap() >= 3, "{}", map["graph"]);

    let _ = std::fs::remove_dir_all(&dir);
}
