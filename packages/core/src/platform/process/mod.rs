//! `process` — o que os pacotes usam para abrir outro programa.
//!
//! [`program`] acha o arquivo de um programa no `PATH` como o sistema o acha,
//! para rodar pelo nome o que no Windows é `.cmd` ou `.bat`.

pub mod program;

pub use program::{command, program_file, program_file_names, program_location};
