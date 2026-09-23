//! O grafo das ondas gravadas no arquivo de eventos de uma spec.
//!
//! Cada onda é um nó e o `depends_on` dela é uma aresta; o nível topológico
//! diz quais saem na mesma rodada, e o ciclo nomeia as ondas que dependem
//! umas das outras em círculo. A dependência e a tarefa que apontam uma onda
//! que o plano não tem ficam fora do grafo: a tarefa está no backlog.
//!
//! Junto do grafo vão os arquivos que as tarefas de cada onda declaram, que
//! a rodada lê para não soltar dois lotes em paralelo sobre o mesmo arquivo.
//! Essa trava mora na rodada, e não aqui: o plano não confere mais ondas que
//! dividem arquivo nem ondas que podiam sair divididas, porque a onda é um
//! lote que o binário forma na hora de despachar, e não um desenho do plano.
//!
//! Nunca entra em pânico: o arquivo sem ondas dá um grafo vazio.

use crate::shared::dag::assign_levels;
use mustard_core::domain::spec_events::{Block, BlockQuery, SpecLog};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Normalização lexical leve de um caminho declarado, para o pareamento entre
/// lotes na rodada: unifica o separador e tira um `./` da frente. De propósito
/// NÃO é uma canonicalização de disco — duas grafias do mesmo arquivo não
/// podem esconder de a rodada que dois lotes mexem nele.
fn normalise_declared_path(raw: &str) -> String {
    let slashed = raw.trim().replace('\\', "/");
    slashed.strip_prefix("./").unwrap_or(&slashed).to_string()
}

/// O grafo das ondas de um plano gravado no arquivo de eventos.
///
/// A leitura é uma só: o nível e o ciclo saem do mesmo peelamento que ordena
/// qualquer grafo do binário. Uma conta própria discordaria dele sobre o
/// mesmo plano.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct WaveGraph {
    /// O nível de despacho de cada onda.
    pub(crate) level: BTreeMap<u64, u32>,
    /// As ondas que estão num ciclo de dependência; vazio num plano são.
    pub(crate) cycle: Vec<u64>,
    /// Os arquivos que as tarefas de cada onda declaram.
    pub(crate) files: BTreeMap<u64, BTreeSet<String>>,
    /// As tarefas de cada onda, pelo código, com os arquivos que cada uma
    /// declara: a onda sem tarefa nenhuma é a que o backlog esvaziou.
    pub(crate) tasks: BTreeMap<u64, Vec<(String, BTreeSet<String>)>>,
}

/// O grafo das ondas do bloco `waves` de uma spec.
pub(crate) fn wave_graph(log: &SpecLog) -> WaveGraph {
    let events = log.block(BlockQuery::Block(Block::Waves));
    let declared: BTreeSet<u64> =
        events.iter().filter(|e| e.event_type == "wave").filter_map(|e| e.wave()).collect();

    // A dependência que aponta uma onda que o plano não tem não vira aresta.
    let mut deps: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for wave in events.iter().filter(|e| e.event_type == "wave") {
        let Some(n) = wave.wave() else { continue };
        let entry = deps.entry(n).or_default();
        entry.extend(wave.ints("depends_on").into_iter().filter(|on| declared.contains(on)));
    }

    let codes = log.codes();
    let mut files: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    // As tarefas de cada onda, com os arquivos de cada uma.
    let mut by_task: BTreeMap<u64, Vec<(String, BTreeSet<String>)>> = BTreeMap::new();
    for task in events.iter().filter(|e| e.event_type == "task") {
        let Some(n) = task.wave() else { continue };
        // A tarefa de uma onda que o plano não tem está no backlog.
        if !declared.contains(&n) {
            continue;
        }
        let code = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
        let mut mine: BTreeSet<String> = BTreeSet::new();
        let entry = files.entry(n).or_default();
        for path in task
            .fields
            .get("files")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|file| file.as_str().or_else(|| file.get("path").and_then(Value::as_str)))
        {
            let path = normalise_declared_path(path);
            if !path.is_empty() {
                entry.insert(path.clone());
                mine.insert(path);
            }
        }
        by_task.entry(n).or_default().push((code, mine));
    }
    let levels = assign_levels(&deps);
    WaveGraph { level: levels.level, cycle: levels.cycle, files, tasks: by_task }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Um arquivo de eventos escrito à mão, uma linha por evento.
    fn spec_log(events: &[(&str, Value)]) -> mustard_core::domain::spec_events::SpecLog {
        use mustard_core::domain::spec_events::{normalize, parse_log, render_line, stamp};
        let mut content = String::new();
        for (i, (event_type, body)) in events.iter().enumerate() {
            let mut map = normalize(body.as_object().cloned().unwrap_or_default(), event_type);
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, i as u64 + 1, None, "2026-09-15T10:00:00-03:00")));
            content.push('\n');
        }
        parse_log(&content)
    }

    fn wave(n: u64, depends_on: &[u64]) -> (&'static str, Value) {
        (
            "wave",
            json!({"n": n, "text": format!("onda {n}"), "criteria": [], "done_when": "passa",
                   "depends_on": depends_on}),
        )
    }

    fn task(n: u64, files: &[&str]) -> (&'static str, Value) {
        let files: Vec<Value> = files.iter().map(|p| json!({"path": p})).collect();
        ("task", json!({"wave": n, "text": format!("tarefa da onda {n}"), "files": files}))
    }

    /// Um ciclo entre ondas nomeia exatamente as ondas do ciclo.
    #[test]
    fn a_loop_between_waves_names_the_waves_on_it() {
        let log = spec_log(&[wave(1, &[]), wave(2, &[3]), wave(3, &[2])]);
        let graph = wave_graph(&log);
        assert_eq!(graph.cycle, vec![2, 3]);
    }

    /// A dependência que aponta uma onda que não existe não vira aresta, e a
    /// tarefa de uma onda que não existe fica fora do grafo: está no backlog.
    #[test]
    fn a_dependency_and_a_task_pointing_at_a_missing_wave_stay_out_of_the_graph() {
        let log = spec_log(&[wave(1, &[7]), task(1, &["src/a.rs"]), task(9, &["src/b.rs"])]);
        let graph = wave_graph(&log);
        assert!(graph.cycle.is_empty());
        assert_eq!(graph.level.keys().copied().collect::<Vec<_>>(), [1]);
        assert!(!graph.tasks.contains_key(&9), "{:?}", graph.tasks);
    }
}
