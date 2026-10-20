//! O estado de cada onda em andamento na resposta da rodada, para quem
//! conduz saber como ela vai sem conferir a cópia à mão.

use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecLog};
use mustard_core::io::wave_prompt::recorded_copy;
use serde_json::{Value, json};

use super::answer::{minutes_since, ref_shown};
use super::queue::silent_minutes;

/// O último passo gravado depois do envio `sent` da onda `wave`: o código do
/// item, pelo mapa `codes`, e o texto. `None` sem passo depois desse envio —
/// um passo de um envio anterior, já respondido, não conta.
fn last_step(log: &SpecLog, wave: u64, sent: u64, codes: &BTreeMap<u64, String>) -> Option<(String, String)> {
    log.block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .rfind(|e| e.event_type == "step" && e.wave() == Some(wave) && e.id > sent)
        .map(|e| (ref_shown(e.fields.get("item"), codes), e.str_field("text").unwrap_or_default().to_string()))
}

/// Os arquivos mudados na cópia da onda `wave`, pelo `git status` curto dela.
/// `None` sem cópia gravada, ou sem git.
///
/// Cada linha do `git status --porcelain` traz dois caracteres de estado, um
/// espaço separador e o caminho — sempre na posição 3. A saída inteira chega
/// trimada (`GitRun::out`), o que só afeta a primeira linha: quando o
/// primeiro caractere de estado dela é espaço (ex.: `" M arquivo"`), o trim
/// da string inteira apaga só esse espaço, e o separador cai uma posição
/// antes. Por isso só a linha de índice 0 é conferida: as outras sempre
/// mantêm a posição 3. O arquivo renomeado (`"velho -> novo"`) dá o caminho
/// novo.
fn copy_files_changed(log: &SpecLog, wave: u64) -> Option<Vec<String>> {
    let copy = recorded_copy(log, wave)?;
    let out = mustard_core::platform::git::run(Path::new(&copy.path), &["status", "--porcelain", "--untracked-files=all"]).out()?;
    Some(
        out.lines()
            .enumerate()
            .filter_map(|(i, line)| {
                let offset = if i == 0 && line.as_bytes().get(2) != Some(&b' ') { 2 } else { 3 };
                line.get(offset..)
            })
            .map(|path| path.rsplit(" -> ").next().unwrap_or(path).trim())
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// O estado da onda `wave` em andamento, para o orquestrador saber como ela
/// vai sem conferir a cópia à mão: o código do envio, os minutos desde ele,
/// os arquivos que a cópia já tem mudados, o último passo gravado depois do
/// envio e os minutos desde o último sinal de vida — a mesma leitura do
/// aviso dos 40 minutos. O que falta fica fora da entrada, sem erro nenhum:
/// nada disto trava a rodada.
pub(super) fn running_state(root: &Path, spec: &str, log: &SpecLog, codes: &BTreeMap<u64, String>, wave: u64, send: &str, sent: u64) -> Value {
    let mut out = json!({ "wave": wave, "send": send });
    if let Some(minutes) = minutes_since(log, sent) {
        out["minutes"] = json!(minutes);
    }
    if let Some(files) = copy_files_changed(log, wave) {
        out["files"] = json!(files);
    }
    if let Some((item, text)) = last_step(log, wave, sent, codes) {
        out["step"] = json!({ "item": item, "text": text });
    }
    if let Some(silent) = silent_minutes(root, spec, wave, log, sent) {
        out["silent_minutes"] = json!(silent);
    }
    out
}
