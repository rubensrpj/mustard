//! Os comandos do fluxo de uma spec, um por passo. Hoje mora aqui o `open`,
//! que abre a spec: a branch, o `spec.ndjson` com o mesmo nome e o nascimento
//! em levantamento. Os passos seguintes do fluxo entram nesta família.

pub mod cli;
pub mod open;
