//! A instalação: a atualização, a faxina das cópias abandonadas e o comando
//! que mede compilando o código certo.

pub mod cli;

pub mod measure;
pub mod scratch_gc;
pub mod upsert;
pub(crate) mod work_copies;
