//! Os comandos do fluxo de uma spec, um por passo. Hoje moram aqui o `open`,
//! que abre a spec: a branch, o `spec.ndjson` com o mesmo nome e o nascimento
//! em levantamento; o `grill`, que grava o tipo de trabalho e monta a lista de
//! pontos do levantamento; e o `reopen`, o caminho de volta, que leva a spec
//! ao levantamento de novo com o motivo gravado; e o `plan`, que monta o
//! pedido de cada onda, confere o plano e leva a spec do levantamento para o
//! plano; e o `round`, a rodada de ondas, que despacha, grava o que voltou,
//! formata e faz o commit. Cada passo responde por [`answer`], que grava a
//! chamada na spec antes de imprimir.

pub mod cli;
pub mod close;
pub mod discard;
pub mod grill;
pub mod open;
pub mod plan;
pub mod reopen;
pub mod resume;
pub mod round;

use std::path::Path;
use std::time::Instant;

use serde_json::Value;

use crate::commands::spec_events::conversation::record_call;

/// A resposta de um passo do fluxo: grava a chamada na spec, imprime o
/// relatório e sai com 1 na recusa. `named` é a spec que a chamada nomeou.
pub(crate) fn answer(command: &str, root: &Path, named: Option<&str>, started: Instant, report: &Value) {
    let _ = record_call(root, command, named, started, report);
    println!("{}", serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".into()));
    if report.get("ok").and_then(Value::as_bool) != Some(true) {
        std::process::exit(1);
    }
}
