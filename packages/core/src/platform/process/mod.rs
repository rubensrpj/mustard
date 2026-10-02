//! `process` — o que os pacotes usam para abrir outro programa.
//!
//! [`rtk_command`] põe o `rtk` na frente de todo programa que o `mustard-rt`
//! e o `mustard-cli` abrem. O RTK é dependência obrigatória do Mustard, e por
//! isso o ajudante não confere se ele existe: o `mustard init` já recusa
//! seguir sem ele. [`program`] acha o arquivo de um programa no `PATH` como o
//! sistema o acha, para rodar pelo nome o que no Windows é `.cmd` ou `.bat`.

pub mod program;
pub mod rtk_command;

pub use program::{command, program_file, program_file_names};
pub use rtk_command::rtk_command;
