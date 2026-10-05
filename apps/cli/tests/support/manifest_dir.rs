//! A pasta do pacote que roda o teste, lida na hora de rodar.
//!
//! O endereço que `env!("CARGO_MANIFEST_DIR")` grava é o da cópia do projeto
//! onde o teste foi compilado. A pasta de compilação passa de uma cópia para
//! outra, e o cargo reaproveita o teste já compilado quando o código não
//! mudou: o endereço gravado segue apontando a cópia antiga, que pode já ter
//! sido apagada. O `cargo test` define a variável ao rodar cada teste, com a
//! pasta do pacote na cópia de agora, e é ela que vale.
//!
//! Um arquivo por pacote: os testes da pasta `tests/` o trazem pelo caminho,
//! e os de dentro de `src/`, quando há, pelo módulo que a raiz do pacote
//! declara só para teste.

use std::path::PathBuf;

/// A pasta do pacote — a do `Cargo.toml` dele — na cópia que roda o teste.
pub fn manifest_dir() -> PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .expect("o cargo test define CARGO_MANIFEST_DIR ao rodar o teste")
}
