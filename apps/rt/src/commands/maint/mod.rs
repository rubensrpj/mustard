//! A instalação: a atualização e a faxina das cópias abandonadas.

pub mod cli;
pub mod config_read;

pub mod scratch_gc;
pub mod upsert;
pub(crate) mod work_copies;
