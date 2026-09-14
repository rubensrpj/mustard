//! Os comandos do fluxo de uma spec, um por passo. Hoje moram aqui o `open`,
//! que abre a spec: a branch, o `spec.ndjson` com o mesmo nome e o nascimento
//! em levantamento; e o `grill`, que grava o tipo de trabalho e monta a lista
//! de pontos do levantamento. Os passos seguintes do fluxo entram nesta
//! família.

pub mod cli;
pub mod grill;
pub mod open;
