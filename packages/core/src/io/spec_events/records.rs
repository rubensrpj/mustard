//! A mudança dos registros de uma spec antiga para o arquivo de operações.
//!
//! As specs de antes gravavam no próprio arquivo de eventos o que os ganchos
//! fizeram, as injeções, as chamadas de comando e o tempo das etapas da
//! rodada: hoje tudo isso nasce no arquivo de operações, e quem lê junta as
//! duas fontes. Numa spec antiga grande, esses registros eram a maior parte do
//! arquivo, lido inteiro a cada passo. Aqui eles saem da spec e vão ao arquivo
//! de operações, só a pedido, com uma cópia de segurança da spec antes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value};

use super::{POINTERS, append_stamped, io_refusal, OPERATIONS_CAP};
use crate::domain::spec_events::{self as model, Refusal, SpecEvent, SpecLog};
use crate::io::fs::lock::LockedFile;
use crate::platform::error::Error;

/// Os tipos do que o programa observa de si e que hoje nascem no arquivo de
/// operações.
const RECORDS: &[&str] = &["hook", "injection", "call", "stage_run"];

/// Os campos do tempo de uma etapa da rodada, o único `stage_run` que nasce
/// no arquivo de operações: a etapa com outro campo — a impressão de uma
/// revisão, o falso positivo, o comando de uma prova — é trabalho que a
/// rodada confere na spec.
const STAGE_TIME: &[&str] = &["v", "id", "code", "at", "type", "author", "phase", "stage", "result", "ms", "scope", "version", "attempt", "search"];

/// O que a mudança tira, ou tiraria, da spec.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Moved {
    /// Quantos eventos saem.
    pub events: usize,
    /// Quantos bytes da spec saem com eles.
    pub bytes: u64,
    /// O tamanho da spec antes da mudança.
    pub spec_bytes: u64,
    /// Quantos eventos saem de cada tipo.
    pub by_type: BTreeMap<String, usize>,
    /// Se a mudança foi feita.
    pub applied: bool,
}

/// O evento `event` de `log` é um registro que pode sair da spec: é de um dos
/// tipos que hoje nascem no arquivo de operações, não é o recibo de leitura
/// de um pedido, não aponta outro item e nenhum outro item o aponta. O tempo
/// de etapa só sai quando é o da rodada, sem nenhum outro campo.
fn movable(event: &SpecEvent, pointed: &BTreeSet<u64>) -> bool {
    if !RECORDS.contains(&event.event_type.as_str()) || pointed.contains(&event.id) {
        return false;
    }
    if event.fields.contains_key("request") || POINTERS.iter().chain(&["reply_to"]).any(|field| event.fields.contains_key(*field)) {
        return false;
    }
    event.event_type != "stage_run"
        || (event.str_field("phase") == Some("delivery")
            && event.fields.contains_key("scope")
            && event.fields.keys().all(|key| STAGE_TIME.contains(&key.as_str())))
}

/// Os números que algum evento de `log` aponta, por qualquer dos campos com
/// que um evento aponta outro.
fn pointed_ids(log: &SpecLog) -> BTreeSet<u64> {
    let mut ids = BTreeSet::new();
    for event in &log.events {
        for field in POINTERS.iter().chain(&["reply_to"]) {
            match event.fields.get(*field) {
                Some(Value::Array(list)) => ids.extend(list.iter().filter_map(Value::as_u64)),
                Some(other) => ids.extend(other.as_u64()),
                None => {}
            }
        }
    }
    ids
}

/// Os campos de `event` sem o número, o código e a hora, que o arquivo de
/// operações dá de novo, e a hora dele.
fn without_envelope(event: &SpecEvent) -> (String, Map<String, Value>, Option<String>) {
    let mut fields = event.fields.clone();
    for key in ["v", "id", "code", "at", "search"] {
        fields.remove(key);
    }
    (event.event_type.clone(), fields, Some(event.at().to_string()).filter(|at| !at.is_empty()))
}

/// Tira da spec `spec` os registros que hoje nascem no arquivo de operações
/// e os acrescenta a `operations`, cada um com a hora dele e o número e o
/// código seguintes do arquivo de operações. Com `backup` `None`, só conta o
/// que sairia, sem tocar em nada. Com ele, a spec inteira vai antes para
/// `backup`, que não pode existir; depois os registros vão ao arquivo de
/// operações e, por último, a spec perde as linhas deles, de uma vez. Tudo com
/// a trava da spec presa: nenhuma gravação entra no meio. `Ok(None)` quando a
/// spec não tem arquivo de eventos.
///
/// # Errors
///
/// [`Refusal::Io`] quando a trava, a leitura, a cópia de segurança ou uma
/// escrita falham. Antes da cópia, nada muda; com a cópia feita, a spec só
/// muda depois de os registros estarem no arquivo de operações.
pub fn move_records(spec: &Path, operations: &Path, backup: Option<&Path>) -> Result<Option<Moved>, Refusal> {
    let mut file = match LockedFile::existing(spec) {
        Ok(file) => file,
        Err(Error::NotFound(_)) => return Ok(None),
        Err(e) => return Err(io_refusal(e)),
    };
    let content = file.read_to_string().map_err(io_refusal)?;
    let log = model::parse_log(&content);
    let pointed = pointed_ids(&log);
    let leaving: BTreeSet<usize> = log.events.iter().filter(|e| movable(e, &pointed)).map(|e| e.line).collect();
    let mut moved = Moved { spec_bytes: u64::try_from(content.len()).unwrap_or(u64::MAX), ..Moved::default() };
    let mut kept = String::with_capacity(content.len());
    for (i, raw) in content.split_inclusive('\n').enumerate() {
        if leaving.contains(&(i + 1)) {
            moved.bytes = moved.bytes.saturating_add(u64::try_from(raw.len()).unwrap_or(u64::MAX));
        } else {
            kept.push_str(raw);
        }
    }
    let records: Vec<&SpecEvent> = log.events.iter().filter(|e| leaving.contains(&e.line)).collect();
    moved.events = records.len();
    for event in &records {
        *moved.by_type.entry(event.event_type.clone()).or_default() += 1;
    }
    let Some(backup) = backup.filter(|_| !records.is_empty()) else {
        return Ok(Some(moved));
    };
    write_backup(backup, &content)?;
    append_stamped(operations, records.iter().map(|e| without_envelope(e)).collect(), |_| Ok(()), Some(OPERATIONS_CAP))?;
    file.replace(kept.as_bytes()).map_err(io_refusal)?;
    if let Some((index, name)) = crate::io::spec_index::index_for(spec) {
        let _ = crate::io::spec_index::refresh_line(&index, &name, &model::parse_log(&kept));
    }
    moved.applied = true;
    Ok(Some(moved))
}

/// Grava `content` em `backup`, que não pode existir, pelo ajudante atômico
/// do disco.
fn write_backup(backup: &Path, content: &str) -> Result<(), Refusal> {
    if crate::io::fs::exists(backup) {
        return Err(Refusal::Io { detail: format!("{}: already exists", backup.display()) });
    }
    if let Some(parent) = backup.parent() {
        crate::io::fs::create_dir_all(parent).map_err(io_refusal)?;
    }
    crate::io::fs::write_atomic(backup, content.as_bytes()).map_err(io_refusal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(value: &Value) -> String {
        format!("{value}\n")
    }

    /// Fica na spec o que é trabalho: o recibo de leitura, a etapa que a
    /// rodada confere, o registro que outro evento aponta; sai o resto, e o
    /// histórico do arquivo de operações fica com cada registro uma vez, com
    /// a hora de quando ele aconteceu.
    #[test]
    fn records_leave_the_spec_and_work_stays() {
        let dir = tempfile::tempdir().unwrap();
        let spec = dir.path().join("spec.ndjson");
        let operations = dir.path().join("ops.ndjson");
        let content = [
            json!({"v":1,"id":1,"at":"2026-09-01T10:00:00-03:00","type":"message","author":"user","text":"a"}),
            json!({"v":1,"id":2,"at":"2026-09-01T10:01:00-03:00","type":"call","author":"binary","command":"round","ms":1,"result":"ok"}),
            json!({"v":1,"id":3,"at":"2026-09-01T10:02:00-03:00","type":"call","author":"binary","command":"read","ms":1,"result":"ok","request":"request-1"}),
            json!({"v":1,"id":4,"at":"2026-09-01T10:03:00-03:00","type":"hook","author":"hook","hook":"h","action":"warn","tool":"Bash","reason":"r"}),
            json!({"v":1,"id":5,"at":"2026-09-01T10:04:00-03:00","type":"remove","author":"assistant","targets":[4],"reason":"x"}),
        ]
        .iter()
        .map(line)
        .collect::<String>();
        std::fs::write(&spec, &content).unwrap();
        std::fs::write(&operations, line(&json!({"v":1,"id":1,"code":"MSTD-CALL-0001","at":"2026-10-01T10:00:00-03:00","type":"call","author":"binary","command":"plan","ms":1,"result":"ok"}))).unwrap();

        let shown = move_records(&spec, &operations, None).unwrap().unwrap();
        assert_eq!((shown.events, shown.applied), (1, false));
        assert_eq!(std::fs::read_to_string(&spec).unwrap(), content);

        let backup = dir.path().join("backup/spec.ndjson");
        let applied = move_records(&spec, &operations, Some(&backup)).unwrap().unwrap();
        assert_eq!((applied.events, applied.applied, applied.by_type.get("call").copied()), (1, true, Some(1)));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), content);
        let left: Vec<u64> = crate::io::spec_events::read(&spec).unwrap().unwrap().events.iter().map(|e| e.id).collect();
        assert_eq!(left, [1, 3, 4, 5]);
        let ops = crate::io::spec_events::read_operations(&operations).unwrap().unwrap();
        let round = ops.events.iter().find(|e| e.str_field("command") == Some("round")).expect("moved");
        assert_eq!((round.id, round.str_field("code"), round.at()), (2, Some("MSTD-CALL-0002"), "2026-09-01T10:01:00-03:00"));

        // Uma cópia de segurança que já existe nunca é sobrescrita.
        std::fs::write(&spec, &content).unwrap();
        assert!(matches!(move_records(&spec, &operations, Some(&backup)), Err(Refusal::Io { .. })));
        assert_eq!(std::fs::read_to_string(&spec).unwrap(), content);
    }
}
