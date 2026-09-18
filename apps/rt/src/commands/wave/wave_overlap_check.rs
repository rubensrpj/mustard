//! `mustard-rt run wave-overlap-check` — advisory PLAN lint for file-scope
//! overlap between dispatch-parallel waves.
//!
//! `dispatch-plan` assigns each wave a topological `level`; waves that share a
//! level have NO dependency between them and are dispatched together in one
//! round (see [`crate::commands::pipeline::dispatch_plan`]). Two such waves that
//! both declare the SAME file in their `## Files` section would put two agents
//! editing that file concurrently with no ordering between them — the "crisp,
//! disjoint boundaries" decomposition discipline exists precisely to prevent
//! this. This audits every dispatch-parallel PAIR and WARNS on a literal file
//! overlap; it NEVER blocks.
//!
//! The signal is OBJECTIVE — a literal set intersection of declared paths, no
//! threshold, no knob. Overlap across DIFFERENT levels is fine (those waves are
//! sequenced by their dependency edge) and is never flagged.
//!
//! Output: one JSON line, mirroring `wave-size-check`'s advisory shape —
//! `{ action, specDir, overlapCount, overlaps: [{ level, waves:[a,b], files:[…],
//! chain }] }`, or `{ action: "skip", reason }` for the not-applicable cases.
//! Deterministic and byte-stable: overlaps ordered by (level, waveA, waveB),
//! files sorted.
//!
//! `chain` is the MINIMAL chaining that zeroes the overlap — one edge, on the
//! higher-numbered wave. Naming only the pair left the reader to work out the
//! repair every time; the pair and its repair travel together now, and they
//! come from [`crate::commands::wave::wave_dependency::same_level_collisions`],
//! the same rule `plan-materialize` refuses on. That sharing is the point: the
//! two steps used to disagree about the same plan, because the one that ran
//! FIRST deduped the shared file away before looking.
//!
//! Fail-open: a missing spec dir, a non-wave spec, or an unreadable wave spec
//! all degrade to a `skip` / empty audit — never a panic, always exit 0.

use crate::shared::dag::assign_levels;
use mustard_core::domain::spec_events::{Block, BlockQuery, SpecLog};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Normalização lexical leve de um caminho declarado, para o pareamento entre
/// ondas: unifica o separador e tira um `./` da frente. De propósito NÃO é uma
/// canonicalização de disco — duas grafias de uma regra só é como um `./` na
/// frente derrubou o portão: a colisão era real no disco e invisível no plano.
fn normalise_declared_path(raw: &str) -> String {
    let slashed = raw.trim().replace('\\', "/");
    slashed.strip_prefix("./").unwrap_or(&slashed).to_string()
}

/// Uma colisão de arquivo entre duas ondas que saem na MESMA rodada, com o
/// encadeamento mínimo que a zera.
///
/// A ordenação é `(level, waves[0], waves[1])` e `files` sai ordenado (vem de
/// uma interseção de `BTreeSet`), então o array é byte-estável.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct FileCollision {
    /// O nível topológico que as duas ondas COMPARTILHAM. Ondas de níveis
    /// diferentes estão sequenciadas pela aresta que as separa e nunca aparecem
    /// aqui.
    pub(crate) level: u32,
    /// As duas ondas, sempre em ordem crescente — a menor primeiro, que é o que
    /// faz o encadeamento apontar sempre para trás.
    pub(crate) waves: [u32; 2],
    /// Os arquivos que AMBAS declaram, ordenados.
    pub(crate) files: Vec<String>,
    /// O encadeamento mínimo que zera a sobreposição, já em prosa acionável:
    /// uma aresta só, na onda de número maior.
    pub(crate) chain: String,
}

impl FileCollision {
    /// Monta a colisão a partir do par já ordenado (`a <= b`) e da interseção.
    ///
    /// `a == b` é o plano que numerou duas ondas igual: o encadeamento por
    /// número não existe aí, então a prescrição nomeia o passo que falta antes.
    fn new(level: u32, a: u32, b: u32, files: Vec<String>) -> Self {
        let chain = if a == b {
            format!(
                "two waves are both numbered {a} — give one of them its own n, then add wave {a} \
                 to that wave's depends_on"
            )
        } else {
            format!("add wave {a} to wave {b}'s depends_on")
        };
        Self { level, waves: [a, b], files, chain }
    }
}

/// Os pares de ondas do MESMO nível de despacho que declaram o mesmo arquivo.
///
/// Cada entrada é `(número que a onda publica, nível topológico, arquivos
/// declarados)`. O conjunto de arquivos entra SEM dedup entre ondas —
/// deduplicar é exatamente o que apaga a evidência da colisão.
fn same_level_collisions(waves: &[(u32, u32, BTreeSet<String>)]) -> Vec<FileCollision> {
    let mut by_level: BTreeMap<u32, Vec<(u32, &BTreeSet<String>)>> = BTreeMap::new();
    for (wave, level, files) in waves {
        by_level.entry(*level).or_default().push((*wave, files));
    }
    let mut out: Vec<FileCollision> = Vec::new();
    for (level, mut group) in by_level {
        group.sort_by_key(|(wave, _)| *wave);
        for i in 0..group.len() {
            for j in (i + 1)..group.len() {
                let shared: Vec<String> = group[i].1.intersection(group[j].1).cloned().collect();
                if !shared.is_empty() {
                    out.push(FileCollision::new(level, group[i].0, group[j].0, shared));
                }
            }
        }
    }
    out
}

/// As partes independentes de uma onda: os grupos de tarefas que não dividem
/// arquivo nenhum entre si, cada um com os códigos das tarefas dele, em ordem.
///
/// Duas tarefas ficam na mesma parte quando declaram um arquivo em comum, e a
/// ligação é transitiva: a tarefa que toca as duas junta as três numa parte
/// só. A tarefa que não declara arquivo nenhum fica de fora — sem arquivo não
/// há como dizer de que parte ela é —, e a onda cujas tarefas se tocam todas
/// sai com uma parte só.
fn independent_parts(tasks: &[(String, BTreeSet<String>)]) -> Vec<Vec<String>> {
    let mut parts: Vec<(Vec<String>, BTreeSet<String>)> = Vec::new();
    for (code, files) in tasks.iter().filter(|(_, files)| !files.is_empty()) {
        // A tarefa junta numa parte só todas as que já tocam um arquivo dela.
        let mut codes = vec![code.clone()];
        let mut touched = files.clone();
        let mut apart: Vec<(Vec<String>, BTreeSet<String>)> = Vec::new();
        for (part_codes, part_files) in parts.drain(..) {
            if part_files.intersection(files).next().is_some() {
                codes.extend(part_codes);
                touched.extend(part_files);
            } else {
                apart.push((part_codes, part_files));
            }
        }
        codes.sort();
        apart.push((codes, touched));
        parts = apart;
    }
    let mut out: Vec<Vec<String>> = parts.into_iter().map(|(codes, _)| codes).collect();
    out.sort();
    out
}

/// O grafo das ondas de um plano gravado no arquivo de eventos.
///
/// As ondas se montam por grafo: cada onda é um nó, o `depends_on` dela é uma
/// aresta, e o nível topológico diz quais saem na mesma rodada. Duas ondas do
/// mesmo nível não têm nada entre elas, então dividir um arquivo ali é dois
/// agentes editando o mesmo arquivo ao mesmo tempo.
///
/// A leitura é uma só: o nível e o ciclo saem do mesmo peelamento que ordena
/// qualquer grafo do binário, e o pareamento de arquivo é o mesmo que a
/// auditoria do plano materializado usa. Duas contas próprias discordariam
/// sobre o mesmo plano.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct WaveGraph {
    /// O nível de despacho de cada onda.
    pub(crate) level: BTreeMap<u64, u32>,
    /// As ondas que estão num ciclo de dependência; vazio num plano são.
    pub(crate) cycle: Vec<u64>,
    /// As ondas que um `depends_on` aponta e que o plano não tem, por onda.
    pub(crate) missing_depends: BTreeMap<u64, Vec<u64>>,
    /// As tarefas que apontam uma onda que o plano não tem: o código da
    /// tarefa e a onda que ela aponta.
    pub(crate) missing_task_waves: Vec<(String, u64)>,
    /// Os pares de ondas do mesmo nível que declaram o mesmo arquivo.
    pub(crate) collisions: Vec<FileCollision>,
    /// As partes independentes das ondas que têm mais de uma: os códigos das
    /// tarefas de cada parte. A onda de uma parte só não entra aqui.
    pub(crate) parts: BTreeMap<u64, Vec<Vec<String>>>,
    /// Os arquivos que as tarefas de cada onda declaram.
    pub(crate) files: BTreeMap<u64, BTreeSet<String>>,
}

/// O grafo das ondas do bloco `waves` de uma spec.
pub(crate) fn wave_graph(log: &SpecLog) -> WaveGraph {
    let events = log.block(BlockQuery::Block(Block::Waves));
    let declared: BTreeSet<u64> =
        events.iter().filter(|e| e.event_type == "wave").filter_map(|e| e.wave()).collect();

    let mut deps: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    let mut missing_depends: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for wave in events.iter().filter(|e| e.event_type == "wave") {
        let Some(n) = wave.wave() else { continue };
        let entry = deps.entry(n).or_default();
        for on in wave.ints("depends_on") {
            if declared.contains(&on) {
                entry.insert(on);
            } else {
                missing_depends.entry(n).or_default().push(on);
            }
        }
    }

    let codes = log.codes();
    let mut missing_task_waves: Vec<(String, u64)> = Vec::new();
    let mut files: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    // Os arquivos de cada tarefa, por onda: é por eles que as partes
    // independentes de uma onda se separam.
    let mut by_task: BTreeMap<u64, Vec<(String, BTreeSet<String>)>> = BTreeMap::new();
    for task in events.iter().filter(|e| e.event_type == "task") {
        let Some(n) = task.wave() else { continue };
        let code = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
        if !declared.contains(&n) {
            missing_task_waves.push((code, n));
            continue;
        }
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
    let parts: BTreeMap<u64, Vec<Vec<String>>> = by_task
        .iter()
        .map(|(n, tasks)| (*n, independent_parts(tasks)))
        .filter(|(_, parts)| parts.len() > 1)
        .collect();

    let levels = assign_levels(&deps);
    let census: Vec<(u32, u32, BTreeSet<String>)> = declared
        .iter()
        .map(|n| {
            let level = levels.level.get(n).copied().unwrap_or(0);
            (u32::try_from(*n).unwrap_or(u32::MAX), level, files.get(n).cloned().unwrap_or_default())
        })
        .collect();

    WaveGraph {
        level: levels.level,
        cycle: levels.cycle,
        missing_depends,
        missing_task_waves,
        collisions: same_level_collisions(&census),
        parts,
        files,
    }
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

    /// Duas ondas que dependem só da primeira saem na mesma rodada; se as duas
    /// declararem o mesmo arquivo, o par aparece com o arquivo e o conserto.
    #[test]
    fn two_waves_in_the_same_round_that_share_a_file_are_paired() {
        let log = spec_log(&[
            wave(1, &[]),
            task(1, &["src/base.rs"]),
            wave(2, &[1]),
            task(2, &["src/shared.rs", "src/a.rs"]),
            wave(3, &[1]),
            task(3, &["src/shared.rs", "src/b.rs"]),
        ]);
        let graph = wave_graph(&log);
        assert!(graph.cycle.is_empty());
        assert_eq!(graph.level[&1], 0);
        assert_eq!(graph.level[&2], graph.level[&3], "sem aresta entre elas, saem juntas");
        assert_eq!(graph.collisions.len(), 1, "{:?}", graph.collisions);
        assert_eq!(graph.collisions[0].waves, [2, 3]);
        assert_eq!(graph.collisions[0].files, vec!["src/shared.rs".to_string()]);
        assert!(graph.collisions[0].chain.contains("depends_on"));
    }

    /// Ondas em níveis diferentes podem dividir arquivo: a aresta entre elas
    /// já as sequencia.
    #[test]
    fn waves_in_different_rounds_may_share_a_file() {
        let log = spec_log(&[wave(1, &[]), task(1, &["src/shared.rs"]), wave(2, &[1]), task(2, &["src/shared.rs"])]);
        assert!(wave_graph(&log).collisions.is_empty());
    }

    /// Um ciclo entre ondas nomeia exatamente as ondas do ciclo.
    #[test]
    fn a_loop_between_waves_names_the_waves_on_it() {
        let log = spec_log(&[wave(1, &[]), wave(2, &[3]), wave(3, &[2])]);
        let graph = wave_graph(&log);
        assert_eq!(graph.cycle, vec![2, 3]);
        assert!(graph.missing_depends.is_empty());
    }

    /// A dependência e a tarefa que apontam uma onda que não existe são
    /// nomeadas, cada uma no seu lugar.
    #[test]
    fn a_dependency_and_a_task_pointing_at_a_missing_wave_are_named() {
        let log = spec_log(&[wave(1, &[7]), task(1, &["src/a.rs"]), task(9, &["src/b.rs"])]);
        let graph = wave_graph(&log);
        assert_eq!(graph.missing_depends.get(&1), Some(&vec![7]));
        assert_eq!(graph.missing_task_waves.len(), 1);
        assert_eq!(graph.missing_task_waves[0].1, 9);
        assert!(graph.missing_task_waves[0].0.contains("TASK"), "{:?}", graph.missing_task_waves);
    }

    /// As partes de uma onda se juntam pela corrente, e não só pelo par: a
    /// primeira tarefa toca `a`, a terceira toca `b`, elas não se tocam, e a
    /// do meio, que toca as duas, junta as três numa parte só. A tarefa de um
    /// arquivo que ninguém mais toca é a outra parte, e a tarefa que não
    /// declara arquivo nenhum não entra em parte nenhuma.
    #[test]
    fn the_parts_of_a_wave_join_through_the_chain_of_shared_files() {
        let log = spec_log(&[
            wave(1, &[]),
            task(1, &["src/a.rs"]),
            task(1, &["src/a.rs", "src/b.rs"]),
            task(1, &["src/b.rs"]),
            task(1, &["src/solta.rs"]),
            task(1, &[]),
        ]);
        let graph = wave_graph(&log);
        let parts = graph.parts.get(&1).cloned().unwrap_or_default();
        assert_eq!(
            parts,
            vec![
                vec![
                    "MSTD-TASK-0001".to_string(),
                    "MSTD-TASK-0002".to_string(),
                    "MSTD-TASK-0003".to_string(),
                ],
                vec!["MSTD-TASK-0004".to_string()],
            ],
            "a corrente junta as três numa parte, e a solta fica na outra"
        );
        assert!(
            !parts.concat().contains(&"MSTD-TASK-0005".to_string()),
            "a tarefa sem arquivo fica de fora: {parts:?}"
        );
    }








}
