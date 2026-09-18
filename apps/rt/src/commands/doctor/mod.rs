//! O diagnóstico e a lista fechada de conferências que o contrato nomeia.
//!
//! Quem não está na lista do contrato não mora mais aqui: a auditoria do
//! catálogo de pastas do `.claude/`, a caça ao `.claude/` aninhado com estado
//! e a busca pela sequência `.claude/.claude/` saíram na onda dos cortes —
//! nenhuma delas é uma das conferências que o contrato nomeia, e o comando só
//! responde pelo que o contrato promete.

// `doctor::doctor` repete o nome do pai de proposito: este modulo E a porta,
// e cada irmao ao lado dele e UMA checagem especifica.
#[allow(clippy::module_inception)]
pub mod doctor;
pub mod bootstrap_check;
pub mod cli;
