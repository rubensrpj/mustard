//! O ensaio das gravações da rodada e a gravação de verdade depois do commit.
//! O ensaio dá um número a cada evento, e um evento da mesma rodada pode
//! apontar outro por ele em `replaces`: a sobra que entra numa tarefa que
//! nasceu ou voltou ao backlog nesta mesma rodada. Entre o ensaio e a
//! gravação roda o commit, e outra gravação pode entrar na spec nesse meio;
//! então a gravação de verdade aponta o número que cada evento recebeu dela,
//! e não o do ensaio.

use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::spec_state::PhaseWriter;
use serde_json::{json, Map, Value};

use crate::commands::spec_events::write::{record, RecordCheck};

/// Uma gravação que o ensaio da rodada conferiu: os campos e o número que o
/// ensaio deu a ela.
pub(super) struct Rehearsed {
    pub(super) id: u64,
    pub(super) draft: Map<String, Value>,
}

/// Confere em `check` a gravação de um evento `event_type` com os campos de
/// `draft` e guarda o número que o ensaio deu a ele.
///
/// # Errors
///
/// A recusa que a gravação daria.
pub(super) fn rehearse(check: &mut RecordCheck, event_type: &str, draft: Map<String, Value>) -> Result<Rehearsed, Refusal> {
    let id = check.record(event_type, draft.clone())?;
    Ok(Rehearsed { id, draft })
}

/// A gravação de verdade do que o ensaio conferiu, na spec `spec` vista de
/// `start`: guarda, de cada evento do ensaio já gravado, o número que a
/// gravação devolveu.
pub(super) struct Recording<'a> {
    start: &'a Path,
    spec: &'a str,
    real: BTreeMap<u64, u64>,
}

impl<'a> Recording<'a> {
    pub(super) fn new(start: &'a Path, spec: &'a str) -> Self {
        Self { start, spec, real: BTreeMap::new() }
    }

    /// Grava `rehearsed` como evento `event_type`, com cada número do
    /// `replaces` (um só ou a lista) que aponta um evento do ensaio já
    /// gravado trocado pelo número que a gravação deu a ele; o que aponta um
    /// evento de antes da rodada fica como veio. Devolve o número gravado e o
    /// `replaces` como foi gravado.
    ///
    /// # Errors
    ///
    /// A recusa da gravação.
    pub(super) fn record(&mut self, event_type: &str, rehearsed: Rehearsed) -> Result<(u64, Option<Value>), Refusal> {
        let Rehearsed { id: rehearsal, mut draft } = rehearsed;
        let real = &self.real;
        let point = |value: &mut Value| {
            if let Some(&id) = value.as_u64().and_then(|n| real.get(&n)) {
                *value = json!(id);
            }
        };
        match draft.get_mut("replaces") {
            Some(Value::Array(items)) => items.iter_mut().for_each(point),
            Some(one) => point(one),
            None => {}
        }
        let replaces = draft.get("replaces").cloned();
        let id = record(self.start, self.spec, event_type, draft, PhaseWriter::Binary)?.written.id;
        self.real.insert(rehearsal, id);
        Ok((id, replaces))
    }
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::scan::ScanReport;
    use mustard_core::io::{project_map, spec_events as store};
    use tempfile::tempdir;

    use super::super::tests::{approved, returned, round, round_with_mine};
    use super::*;

    /// Uma resposta gravada na spec entre o ensaio e a gravação de verdade,
    /// depois do commit, faz o número de cada evento andar. A volta que deixa
    /// a tarefa por fazer e acha uma sobra no mesmo arquivo dela é assumida
    /// mesmo assim: a versão da tarefa com a sobra aponta a versão que a
    /// devolveu ao backlog, e não a resposta que tomou o número do ensaio.
    #[test]
    fn a_write_between_the_rehearsal_and_the_recording_keeps_the_leftover_on_the_returned_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let map = json!({"modules": [{"path": "src/a.rs", "language": "rust", "deps": []}]});
        project_map::write_text(root, &map.to_string()).unwrap();
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let task = log.visible().into_iter().find(|e| e.event_type == "task").map(|e| e.id).unwrap();
        let code = log.codes()[&task].clone();

        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma saiu pela metade.\n").unwrap();
        let body = json!({"wave": 1, "text": "A soma saiu pela metade.", "files": ["src/a.rs"],
            "commit": "a soma sai pela metade", "undone": [code],
            "leftovers": [{"title": "A soma perde o sinal", "detail": "Em `src/a.rs`, o sinal some."}]});
        assert_eq!(returned(root, body)["ok"], json!(true));

        let said = "Uma resposta no meio da rodada.";
        let mine = |root: &Path, out: &Path| {
            crate::shared::spec_state::seed_event(root, "x", "message", json!({"author": "user", "text": said}));
            project_map::write_text_at(out, &map.to_string()).unwrap();
            Ok(ScanReport::default())
        };
        let out = round_with_mine(root, "x", None, &mine);
        assert_eq!(out["ok"], json!(true), "a gravação no meio não derruba a rodada: {out}");
        let tasks: Vec<&Value> = out["recorded"].as_array().unwrap().iter().filter(|r| r["type"] == json!("task")).collect();
        let back = tasks.iter().find(|r| r.get("replaces").is_none()).unwrap_or_else(|| panic!("{out}"));
        let joined = tasks.iter().find(|r| r.get("replaces").is_some()).unwrap_or_else(|| panic!("{out}"));
        assert_eq!(joined["replaces"], back["id"], "a sobra entra na tarefa devolvida: {out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let alien = log.visible().into_iter().find(|e| e.str_field("text") == Some(said)).map(|e| e.id).unwrap();
        assert!(alien < back["id"].as_u64().unwrap(), "a resposta entrou antes da gravação de verdade: {out}");
        let version = log.get(joined["id"].as_u64().unwrap()).unwrap();
        assert_eq!(version.fields.get("replaces"), Some(&back["id"]), "{out}");
        assert!(version.str_field("agent").is_some_and(|agent| agent.contains("A soma perde o sinal")), "{out}");
    }
}
