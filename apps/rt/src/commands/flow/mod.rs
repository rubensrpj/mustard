//! Os comandos do fluxo de uma spec, um por passo. Hoje moram aqui o `open`,
//! que abre a spec: a branch, o `spec.ndjson` com o mesmo nome e o nascimento
//! em levantamento; o `grill`, que grava o tipo de trabalho e monta a lista de
//! pontos do levantamento; e o `reopen`, o caminho de volta, que leva a spec
//! ao levantamento de novo com o motivo gravado; e o `plan`, que monta o
//! pedido de cada onda, confere o plano e leva a spec do levantamento para o
//! plano; e o `round`, a rodada de ondas, que despacha, grava o que voltou,
//! formata e faz o commit. Os passos seguintes do fluxo entram nesta família.

pub mod cli;
pub mod close;
pub mod discard;
pub mod grill;
pub mod open;
pub mod plan;
pub mod reopen;
pub mod resume;
pub mod round;
