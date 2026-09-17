//! A fila da rodada e as ondas em andamento: quais ondas saem agora, quais
//! estão em andamento, quais esperam revisão, quais já estão entregues e
//! aprovadas, e o estado de cada uma que a página mostra.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog};
use serde_json::{json, Value};

use super::stops::waves_replanned;
use crate::commands::wave::wave_overlap_check::wave_graph;

/// Quantas ondas saem juntas quando o projeto não diz outra coisa: duas, que é
/// quanto a máquina aguenta compilando ao mesmo tempo.
const DEFAULT_PARALLEL: usize = 2;

/// Quantas ondas o projeto deixa compilar ao mesmo tempo.
pub(super) fn max_parallel(root: &Path) -> usize {
    mustard_core::ProjectConfig::load(root).max_compiling_waves().unwrap_or(DEFAULT_PARALLEL)
}

/// As ondas que saem nesta rodada: as que ainda não saíram nem entregaram,
/// cujas dependências já foram entregues, no máximo `limit` junto com as que
/// estão em andamento (`running`), e nunca duas que declaram o mesmo arquivo —
/// duas ondas assim seriam dois agentes editando o mesmo arquivo ao mesmo
/// tempo. A onda em andamento conta como uma que já saiu nesta rodada: ocupa
/// uma vaga e reserva os arquivos das tarefas dela. A onda parada pelo limite
/// de consertos (`stuck`) não sai, nem a que depende dela, direta ou por outra
/// onda.
pub(super) fn next_waves(
    log: &SpecLog,
    limit: usize,
    running: &BTreeMap<u64, u64>,
    stuck: &BTreeMap<u64, Vec<&SpecEvent>>,
) -> Vec<u64> {
    let graph = wave_graph(log);
    // A onda reprovada volta para a fila: sem isso o ciclo de conserto não
    // fecha, porque o fechamento recusa e diz qual refazer e a rodada nunca a
    // despacharia de novo.
    let to_redo = waves_to_redo(log);
    // O pedido gravado descreve o plano daquele momento: a onda que ganhou
    // versão nova depois dele, e ainda não entregou, sai de novo com o pedido
    // do plano atual.
    let replanned = waves_replanned(log);
    // O que já saiu da fila: a onda com pedido e também a onda que já
    // entregou. Só o pedido não basta, porque a onda entregue antes de a
    // rodada existir não tem pedido nenhum e apareceria como pronta para
    // sair — e sairia de novo um trabalho já feito.
    // Quais ondas já entregaram é pergunta do núcleo, e é ele que responde:
    // recalcular o filtro aqui deixava duas camadas decidindo a mesma coisa,
    // concordando hoje e livres para divergir amanhã.
    let delivered = log.delivered_waves();
    let already_out: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "send")
        .filter_map(SpecEvent::wave)
        .filter(|n| !replanned.contains(n))
        .chain(delivered.iter().copied())
        .filter(|n| !to_redo.contains(n))
        .collect();
    let mut depends: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for wave in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "wave") {
        if let Some(n) = wave.wave() {
            depends.insert(n, wave.ints("depends_on"));
        }
    }
    let files = task_files(log);
    let done = waves_done(log, running);
    let slots = limit.saturating_sub(running.len());
    let mut out: Vec<u64> = Vec::new();
    let mut taken: BTreeSet<String> =
        running.keys().flat_map(|n| files.get(n).cloned().unwrap_or_default()).collect();
    for n in ready_in_order(&graph, &depends, &already_out, &delivered, &done) {
        if out.len() >= slots {
            break;
        }
        if stuck.contains_key(&n) || dependencies_of(n, &depends).iter().any(|d| stuck.contains_key(d)) {
            continue;
        }
        let declared = files.get(&n).cloned().unwrap_or_default();
        if declared.iter().any(|f| taken.contains(f)) {
            continue;
        }
        taken.extend(declared);
        out.push(n);
    }
    out
}

/// Os arquivos que as tarefas de cada onda declaram.
pub(super) fn task_files(log: &SpecLog) -> BTreeMap<u64, BTreeSet<String>> {
    let mut files: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    for task in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "task") {
        let Some(n) = task.wave() else { continue };
        let entry = files.entry(n).or_default();
        for file in task
            .fields
            .get("files")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|f| f.as_str().or_else(|| f.get("path").and_then(Value::as_str)))
        {
            entry.insert(file.replace('\\', "/"));
        }
    }
    files
}

/// As ondas em andamento, cada uma com o número do pedido dela: a onda tem
/// pedido e nenhuma entrega depois dele. O pedido mais antigo que a versão
/// mais nova da onda ou de uma tarefa dela descreve um plano que já mudou, e
/// não conta. Uma onda que já entregou só volta a sair por uma reprovação: o
/// pedido que veio depois de uma entrega, sem reprovação entre as duas, não é
/// trabalho em curso, nem o pedido de uma onda que saiu do plano.
pub(crate) fn waves_in_progress(log: &SpecLog) -> BTreeMap<u64, u64> {
    let planned = log.planned_waves();
    let replanned = waves_replanned(log);
    let verdicts = log.verdicts_by_wave();
    let mut deliveries: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for delivered in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "delivered") {
        if let Some(n) = delivered.wave() {
            deliveries.entry(n).or_default().push(delivered.id);
        }
    }
    log.last_by_wave("send")
        .into_iter()
        .filter(|(n, _)| planned.contains(n) && !replanned.contains(n))
        .filter(|(n, sent)| {
            let ids = deliveries.get(n).map(Vec::as_slice).unwrap_or_default();
            if ids.iter().any(|id| id > sent) {
                return false;
            }
            let judged_before = verdicts
                .get(n)
                .and_then(|list| list.iter().rev().find(|v| v.id < *sent))
                .and_then(|v| v.str_field("result"));
            !ids.iter().any(|id| id < sent) || judged_before == Some("rejected")
        })
        .collect()
}

/// As ondas entregues e aprovadas: têm entrega, não estão em andamento, não
/// esperam revisão, e a última revisão delas não reprovou. A onda entregue
/// antes de a rodada existir, sem pedido e sem veredito, está provada pelo
/// código que entrou.
fn waves_done(log: &SpecLog, running: &BTreeMap<u64, u64>) -> BTreeSet<u64> {
    let awaiting: BTreeSet<u64> = waves_awaiting_review(log).into_iter().collect();
    let rejected = log.last_rejected();
    log.delivered_waves()
        .into_iter()
        .filter(|n| !running.contains_key(n) && !awaiting.contains(n) && !rejected.contains_key(n))
        .collect()
}

/// A primeira onda planejada que ainda não está entregue e aprovada, com as
/// em andamento em `running`. `None` quando todas estão.
pub(super) fn first_unfinished(log: &SpecLog, running: &BTreeMap<u64, u64>) -> Option<u64> {
    let done = waves_done(log, running);
    log.planned_waves().into_iter().find(|n| !done.contains(n))
}

/// A onda `n` depende de todas as outras que ainda não terminaram: as
/// dependências dela, diretas ou por outra onda, alcançam cada onda planejada
/// que não está entregue e aprovada. É a última onda da obra.
fn depends_on_all(n: u64, depends: &BTreeMap<u64, Vec<u64>>, done: &BTreeSet<u64>) -> bool {
    let reached = dependencies_of(n, depends);
    depends.keys().filter(|w| **w != n && !done.contains(w)).all(|w| reached.contains(w))
}

/// As ondas de que `n` depende, diretas ou por outra onda.
fn dependencies_of(n: u64, depends: &BTreeMap<u64, Vec<u64>>) -> BTreeSet<u64> {
    let mut reached: BTreeSet<u64> = BTreeSet::new();
    let mut stack: Vec<u64> = depends.get(&n).cloned().unwrap_or_default();
    while let Some(on) = stack.pop() {
        if reached.insert(on) {
            stack.extend(depends.get(&on).cloned().unwrap_or_default());
        }
    }
    reached
}

/// As ondas que voltam para a fila: a última revisão delas reprovou, e o
/// conserto ainda não saiu — o pedido mais novo da onda e a entrega mais nova
/// dela são anteriores a essa reprovação. Depois que o conserto sai, a onda espera a revisão dele, e não
/// é despachada de novo pela mesma reprovação.
pub(super) fn waves_to_redo(log: &SpecLog) -> BTreeSet<u64> {
    let last_send = log.last_by_wave("send");
    let delivered = log.last_by_wave("delivered");
    log.last_rejected()
        .into_iter()
        .filter(|(n, id)| last_send.get(n).is_none_or(|sent| sent < id))
        .filter(|(n, id)| delivered.get(n).is_none_or(|fix| fix < id))
        .map(|(n, _)| n)
        .collect()
}

/// As ondas prontas para sair, em ordem de nível e de número. `already_out`
/// são as ondas que já saíram da fila e `delivered` as que já entregaram, que
/// é o que solta as ondas dependentes delas. A onda que depende de todas as
/// outras só sai com as dependências entregues e aprovadas (`done`).
fn ready_in_order(
    graph: &crate::commands::wave::wave_overlap_check::WaveGraph,
    depends: &BTreeMap<u64, Vec<u64>>,
    already_out: &BTreeSet<u64>,
    delivered: &BTreeSet<u64>,
    done: &BTreeSet<u64>,
) -> Vec<u64> {
    let mut ready: Vec<(u32, u64)> = depends
        .iter()
        .filter(|(n, _)| !already_out.contains(n))
        .filter(|(n, on)| {
            let released = if depends_on_all(**n, depends, done) { done } else { delivered };
            on.iter().all(|d| released.contains(d))
        })
        .map(|(n, _)| (graph.level.get(n).copied().unwrap_or(0), *n))
        .collect();
    ready.sort_unstable();
    ready.into_iter().map(|(_, n)| n).collect()
}

/// As revisões que esta rodada pede: uma por onda cuja entrega mais nova é
/// posterior ao veredito mais novo, pela regra de [`waves_awaiting_review`],
/// com o pedido do revisor já montado — a lista de itens da onda, os
/// critérios e os defeitos já vistos naqueles arquivos.
pub(super) fn reviews_due(log: &SpecLog, built: &[mustard_core::io::wave_prompt::WavePrompt]) -> Vec<Value> {
    waves_awaiting_review(log)
        .into_iter()
        .map(|wave| {
            let review = built.iter().find(|p| p.wave == wave).map(|p| p.review.clone()).unwrap_or_default();
            json!({ "wave": wave, "prompt": review })
        })
        .collect()
}

/// As ondas cuja entrega mais nova é posterior ao veredito mais novo: a
/// revisão delas é o que a rodada seguinte pede. Entra a onda que nunca foi
/// revisada, por não ter veredito nenhum, e entra também a onda reprovada que
/// já entregou o conserto — o conserto é mais novo que a reprovação. Excluir
/// toda onda que tem veredito fechava a porta da segunda: o conserto nunca
/// voltava para a revisão e o fechamento recusava para sempre, porque o último
/// veredito seguia sendo o que reprovou.
///
/// Sem veredito nenhum, só pede revisão a entrega que responde a um pedido da
/// rodada — a entrega mais nova que o pedido mais novo daquela onda. A onda
/// entregue antes de a rodada existir não tem pedido nenhum, e cobrar revisão
/// dela é cobrar de novo um trabalho já feito, provado pelo código que entrou.
/// A onda que saiu do plano não é revisada.
fn waves_awaiting_review(log: &SpecLog) -> Vec<u64> {
    let planned = log.planned_waves();
    let last_verdict: BTreeMap<u64, u64> =
        log.verdicts_by_wave().into_iter().filter_map(|(n, verdicts)| verdicts.last().map(|v| (n, v.id))).collect();
    let last_send = log.last_by_wave("send");
    log.last_by_wave("delivered")
        .into_iter()
        .filter(|(n, _)| planned.contains(n))
        .filter(|(n, id)| last_verdict.get(n).is_none_or(|judged| judged < id))
        .filter(|(n, id)| last_verdict.contains_key(n) || last_send.get(n).is_some_and(|sent| sent < id))
        .map(|(n, _)| n)
        .collect()
}

/// Os itens que o pedido de uma onda leva: os números de tudo que entrou nele.
pub(super) fn sent_items(log: &SpecLog, wave: u64) -> Vec<u64> {
    log.step(&mustard_core::domain::spec_events::Step::Dispatch { wave })
        .into_iter()
        .map(|e| e.id)
        .collect()
}

/// O estado de cada onda que já saiu, pela mesma leitura que decide o que a
/// rodada despacha: em andamento, entregue à espera da revisão, reprovada na
/// última revisão ou entregue e aprovada. A onda que não está aqui está por
/// fazer. A página da spec mostra este estado.
pub(crate) fn wave_states(log: &SpecLog) -> mustard_core::view::document::WaveStates {
    use mustard_core::view::document::WaveState;
    let running = waves_in_progress(log);
    let awaiting: BTreeSet<u64> = waves_awaiting_review(log).into_iter().collect();
    let rejected = log.last_rejected();
    let done = waves_done(log, &running);
    let mut states = mustard_core::view::document::WaveStates::new();
    for n in running.keys().chain(&awaiting).chain(rejected.keys()).chain(&done) {
        let state = if running.contains_key(n) {
            WaveState::Running
        } else if awaiting.contains(n) {
            WaveState::Delivered
        } else if rejected.contains_key(n) {
            WaveState::Rejected
        } else {
            WaveState::Approved
        };
        states.insert(*n, state);
    }
    states
}

#[cfg(test)]
mod tests {
    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// Duas ondas sem dependência que mexem no mesmo arquivo nunca saem
    /// juntas, e o teto de compilações do projeto limita quantas saem.
    #[test]
    fn two_waves_never_go_out_together_when_they_share_a_file_and_the_cap_holds() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);

        let out = round(root, "x", None);
        let waves: Vec<u64> =
            out["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(waves, vec![1, 3], "a onda 2 divide arquivo com a 1: {out}");

        // Com o teto do projeto em 1, só uma onda sai por rodada.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        let out = round(root, "x", None);
        assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{out}");
    }

    /// A onda que já entregou não é despachada de novo, mesmo sem pedido
    /// nenhum: a onda entregue antes de a rodada existir não tem pedido, e
    /// mandá-la sair seria mandar refazer um trabalho já feito.
    #[test]
    fn a_wave_that_already_delivered_does_not_go_out_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        write(
            root,
            "x",
            "delivered",
            json!({"wave": 1, "text": "A onda 1 saiu antes da rodada.", "files": ["src/a.rs"]}),
        );

        let out = round(root, "x", None);
        let waves: Vec<u64> =
            out["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(waves, vec![2], "a onda 1 já tem entrega: {out}");
    }

    /// A onda entregue antes de a rodada existir também não entra na lista de
    /// revisões: sem veredito nenhum e sem pedido, a entrega dela não responde
    /// a nada que esta rodada tenha mandado fazer.
    #[test]
    fn a_wave_delivered_before_the_round_is_not_asked_for_review() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        write(
            root,
            "x",
            "delivered",
            json!({"wave": 1, "text": "A onda 1 saiu antes da rodada.", "files": ["src/a.rs"]}),
        );

        let out = round(root, "x", None);
        assert_eq!(out["reviews"], json!([]), "a onda 1 entregou antes e nunca foi pedida: {out}");
    }

    /// A onda cuja última revisão reprovou volta a ser despachada, e uma vez
    /// só: depois que o conserto sai, a mesma reprovação não a manda de novo.
    /// Entregue o conserto, ele volta para a revisão — é o que fecha o ciclo,
    /// porque sem uma revisão nova o veredito que reprovou valeria para sempre.
    #[test]
    fn a_rejected_wave_goes_out_again_and_only_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(1), "{first}");

        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        let again = round(root, "x", Some(&verdict(1, "rejected", "faltou o teste")));
        assert_eq!(again["dispatch"].as_array().map(Vec::len), Some(1), "a onda reprovada volta a sair: {again}");

        let quiet = round(root, "x", None);
        assert_eq!(quiet["dispatch"], json!([]), "o conserto já saiu, e a onda espera a revisão dele: {quiet}");
        assert_eq!(quiet["reviews"], json!([]), "o conserto ainda não voltou: nada a revisar: {quiet}");

        // O conserto entregue é mais novo que a reprovação, e por isso pede
        // revisão: é a revisão nova que tira o veredito velho da frente.
        let back = round(root, "x", Some(&delivered(root, 1, "O teste entrou.", &["src/a.rs"])));
        let waves: Vec<u64> = back["reviews"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|review| review["wave"].as_u64())
            .collect();
        assert_eq!(waves, vec![1], "o conserto entregue volta para a revisão: {back}");
    }

    /// O estado de cada onda que a página mostra acompanha a rodada: em
    /// andamento depois do pedido, entregue à espera da revisão, reprovada
    /// pela última revisão, em andamento de novo com o conserto e aprovada no
    /// fim; a onda que ainda não saiu não aparece, e a página a mostra por
    /// fazer.
    #[test]
    fn the_wave_states_follow_the_round() {
        use mustard_core::view::document::WaveState::{Approved, Delivered, Rejected, Running};
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        let states = || {
            let log = mustard_core::io::spec_events::read(&root.join(".claude/spec/x/spec.ndjson")).unwrap().unwrap();
            wave_states(&log).into_iter().collect::<Vec<_>>()
        };
        assert_eq!(states(), [], "nothing went out yet");
        round(root, "x", None);
        assert_eq!(states(), [(1, Running)]);
        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(states(), [(1, Delivered)], "the delivery waits for its review, and the last wave waits for it");
        // A reprovação gravada antes de a rodada seguinte despachar o conserto.
        let events = root.join(".claude/spec/x/spec.ndjson");
        let log = mustard_core::io::spec_events::read(&events).unwrap().unwrap();
        let crit = log.events.iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        let rejected = json!({"author": "review", "wave": 1, "result": "rejected", "text": "faltou o teste",
            "criteria": [{"criterion": crit, "tests_rule": true}]});
        mustard_core::io::spec_events::write(&events, "verdict", rejected.as_object().cloned().unwrap(), &[]).unwrap();
        assert_eq!(states(), [(1, Rejected)]);
        round(root, "x", None);
        assert_eq!(states(), [(1, Running)], "the fix went out");
        round(root, "x", Some(&delivered(root, 1, "O teste entrou.", &["src/a.rs"])));
        round(root, "x", Some(&verdict(1, "approved", "pronto")));
        assert_eq!(states(), [(1, Approved), (2, Running)]);
    }

    /// A onda em andamento — com pedido e sem entrega depois dele — ocupa uma
    /// vaga do limite e reserva os arquivos das tarefas dela: a rodada não
    /// solta a onda que divide arquivo com ela, nem passa do limite contando
    /// as que já saíram. O pedido anterior ao replanejamento da onda não conta
    /// como andamento. A resposta lista as ondas em andamento com o código do
    /// pedido de cada uma.
    #[test]
    fn a_wave_in_flight_holds_a_slot_and_its_files_and_a_send_before_the_replan_does_not_count() {
        // A onda 2 divide arquivo com a 1, que está em andamento.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 3], "{first}");
        let out = round(root, "x", Some(&delivered(root, 3, "Saiu.", &["src/c.rs"])));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "a onda 2 divide arquivo com a 1: {out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let sent = log
            .visible()
            .into_iter()
            .find(|e| e.event_type == "send" && e.wave() == Some(1))
            .map(|e| codes[&e.id].clone())
            .unwrap();
        assert_eq!(out["running"], json!([{"wave": 1, "send": sent}]), "{out}");

        // Duas ondas em andamento enchem o limite de duas.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        let full = round(root, "x", None);
        assert_eq!(waves_in(&full, "dispatch"), Vec::<u64>::new(), "as duas vagas estão ocupadas: {full}");
        assert_eq!(waves_in(&full, "running"), vec![1, 2], "{full}");

        // Replanejada depois do pedido, a onda 2 deixa de estar em andamento:
        // ela sai de novo, e a vaga dela não fica presa ao pedido velho.
        replan(root, 2);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![2], "{again}");
        let running = again["running"].as_array().cloned().unwrap_or_default();
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let newest = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "send" && e.wave() == Some(2))
            .map(|e| codes[&e.id].clone())
            .next_back()
            .unwrap();
        assert_eq!(running[1], json!({"wave": 2, "send": newest}), "o pedido novo é o que conta: {again}");

        // A onda 1 entregou antes de a rodada existir, e um pedido saiu para
        // ela depois, sem reprovação no meio: ela não está em andamento, e as
        // duas vagas ficam para as outras.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);
        write(root, "x", "delivered", json!({"wave": 1, "text": "Saiu antes da rodada.", "files": ["src/a.rs"]}));
        seed_send(root, 1);
        let free = round(root, "x", None);
        assert_eq!(waves_in(&free, "dispatch"), vec![2, 3], "{free}");
        assert_eq!(waves_in(&free, "running"), vec![2, 3], "a onda 1 não está em andamento: {free}");
    }

    /// A onda que depende de todas as outras só sai depois das aprovações
    /// delas, não só das entregas. A que depende de uma parte sai com a
    /// entrega, como antes.
    #[test]
    fn the_wave_that_depends_on_all_the_others_waits_for_their_approvals() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[1, 2])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);

        let both = format!("{}\n{}", delivered(root, 1, "Saiu.", &["src/a.rs"]), delivered(root, 2, "Saiu.", &["src/b.rs"]));
        let out = round(root, "x", Some(&both));
        assert_eq!(waves_in(&out, "reviews"), vec![1, 2], "{out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "entregues não bastam: {out}");

        let one = round(root, "x", Some(&verdict(1, "approved", "passou")));
        assert_eq!(one["ok"], json!(true), "{one}");
        assert_eq!(waves_in(&one, "dispatch"), Vec::<u64>::new(), "a onda 2 ainda espera revisão: {one}");
        let both = round(root, "x", Some(&verdict(2, "approved", "passou")));
        assert_eq!(waves_in(&both, "dispatch"), vec![3], "as duas aprovadas soltam a 3: {both}");

        // A onda 2 depende só da 1, e a 3 não passa por ela: a 2 sai com a
        // entrega da 1.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1]), (3, &["src/c.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 3]);
        let out = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "a entrega basta para quem não depende de todas: {out}");
    }
}
