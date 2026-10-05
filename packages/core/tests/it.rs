//! Os testes de integração deste pacote, num programa só: cada arquivo de
//! `tests/` é um módulo daqui. Um programa por arquivo custava uma ligação, um
//! início de processo e uma pasta de compilação por arquivo.
//!
//! Arquivo novo em `tests/` entra nesta lista com uma linha `mod <nome>;`: a
//! descoberta automática está desligada (`autotests = false` no
//! `Cargo.toml`), e o arquivo fora da lista não roda.

// Cada arquivo traz os ajudantes dele pelo caminho (`#[path = "support/..."]`):
// o mesmo arquivo entra como módulo de vários arquivos. Assim cada um conserva
// o `cfg` e só o que usa, em todos os sistemas, como quando era um programa à
// parte.
#![allow(clippy::duplicate_mod)]

// `support/prose_budget.rs` lê a pasta do pacote por aqui, pela raiz do programa.
#[path = "support/manifest_dir.rs"]
mod manifest_dir;

mod integration_programs;
mod layers_hold;
mod private_install;
mod private_install_leaves_no_trace;
mod search_while_writing;
mod seeded_ignore;
mod template_budget;
mod version_line;
mod workflows;
