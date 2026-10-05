//! A montagem das ondas a partir do backlog: a cada rodada, as ondas que saem
//! agora, no máximo uma por vaga livre, cada uma com um assunto só, e a
//! desmontagem da onda de lote que ficou montada e não saiu. Só existe a onda
//! que está rodando; o resto das tarefas fica no backlog, sem número de onda.
//! O assunto é o tipo de trabalho que o Jev julga numa chamada só sobre o
//! backlog inteiro; sem o Jev, ou com a chamada falhando, é o arquivo que as
//! tarefas dividem.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog};
use mustard_core::domain::map_filter::FilterError;
use mustard_core::domain::spec_state::PhaseWriter;
use serde_json::{json, Map, Value};

use super::leftovers::is_cleanup;
use super::queue::{
    backlog_left, backlog_left_without, backlog_population, backlog_ready, backlog_ready_without, covers_nothing,
    open_sends, outgoing, task_revision, waves_done, waves_in_progress,
};
use super::report::backlog_return;
use super::stops::waves_stuck;
use super::summary_wave::summary_waves;
use crate::commands::spec_events::conversation::record_measured_call;
use crate::commands::spec_events::write::{record, RecordCheck};
use crate::commands::wave::wave_overlap_check::wave_graph;
use crate::hooks::session::conversation_size::WAVE_LIMIT;
use crate::shared::dag::{pack_by_kind, sets_cross, touches_whole_tree, BacklogTask, Reserved};
use crate::shared::jev::{Board, BoardTask, BoardWave, Judged};
use crate::shared::task_size::wave_budget;

/// Quem julga o backlog para a montagem: uma chamada só, com o quadro inteiro
/// ([`Board`]), que volta com o tipo de trabalho de cada tarefa, o tamanho
/// dela e o quanto ela pode mudar o mesmo que uma onda em andamento
/// ([`Judged`]). A chamada que falha deixa a montagem pelo arquivo, como sem o
/// julgamento.
pub(crate) type Judge<'a> = dyn Fn(&Board) -> Result<Judged, FilterError> + 'a;

/// Monta as ondas que saem nesta rodada, olhando o backlog inteiro
/// ([`backlog_left`], [`backlog_ready`]): no máximo uma por vaga livre de
/// `limit` (as vagas do projeto, `max_parallel`), cada uma com um assunto só,
/// sem corte em número de tarefas e sem juntar assuntos sem relação. O que não
/// cabe nas vagas fica no backlog, sem onda, e entra na montagem da rodada em
/// que uma vaga abrir. A limpeza ([`is_cleanup`]) só entra quando nada mais
/// resta ([`backlog_ready`]).
///
/// O assunto vem do Jev (`judge`), numa chamada só por montagem, feita só
/// quando há vaga livre e tarefa pronta: o quadro leva as ondas em andamento
/// e o backlog pronto, e a resposta diz o tipo de trabalho de cada tarefa, o
/// tamanho dela e o quanto ela muda o mesmo que cada onda em andamento. A onda
/// junta as tarefas do mesmo tipo, tenham ou não arquivo em comum, até a soma
/// do tamanho delas chegar ao que um agente faz antes do limite da conversa
/// dele (`WAVE_LIMIT`, menos o começo da conversa; [`wave_budget`]): o que
/// passa disso fecha a onda e vai para outra, e a tarefa que sozinha passa do
/// teto sai sozinha. A de tipo incerto sai sozinha, e a que muda o mesmo que
/// uma onda aberta espera no backlog. As ondas saem na ordem fixa dos tipos —
/// defeito primeiro, limpeza no fim — e, dentro do tipo, pelo número
/// ([`pack_by_kind`]). Duas ondas com arquivo em comum nunca saem juntas, de
/// tipos diferentes ou não: a que perde a vez fica no backlog. A chamada grava um evento `call` com os tokens, o custo e
/// o modelo, como a busca. Sem `judge` (sem chave, ou o Jev desligado), ou com
/// a chamada falhando, o assunto é o arquivo: as tarefas que dividem arquivo,
/// direto ou por uma corrente de outras, e a de código mais baixo sai
/// primeiro ([`pack_batches`](crate::shared::dag::pack_batches)). Em qualquer
/// dos dois, a tarefa com o curinga da árvore inteira só sai sozinha, sem nada
/// em andamento.
///
/// A onda pequena espera juntar trabalho: o lote com menos de
/// [`MIN_WAVE_FILES`](crate::shared::dag::MIN_WAVE_FILES) arquivos declarados
/// não sai enquanto houver onda em andamento ([`waves_in_progress`]). Sai
/// quando chega a esse tamanho — com as tarefas do mesmo tipo que o Jev juntou
/// nele — ou quando nada roda. A onda que continua um resumo que vale e a
/// tarefa do curinga da árvore inteira não esperam. O lote que espera reserva
/// os arquivos dele, como a tarefa bloqueada: a que vem depois e os divide
/// também espera.
///
/// A tarefa que não cobre item nenhum ([`covers_nothing`]) fica de fora das
/// duas montagens, como pronta e como a que espera: a onda leva os critérios
/// que as tarefas dela cobrem, e a gravação da onda sem nenhum recusa a
/// rodada inteira, não só ela. Ela segue no backlog sem onda, e
/// [`backlog_uncovered`](super::queue::backlog_uncovered) a entrega à rodada,
/// que a nomeia num aviso até uma versão dela trazer o `covers`.
///
/// Só existe a onda que está rodando. A onda de lote montada e não enviada de
/// uma rodada anterior — a que não ganhou cópia para sair, por exemplo — é
/// desfeita: cada tarefa dela volta ao backlog ([`backlog_return`]) e entra na
/// montagem junto das outras, e a que cai num assunto montado agora ganha uma
/// versão só, com o número da onda nova.
///
/// Cada onda grava o evento de onda, com autor binário: os critérios são a
/// união do que as tarefas dela cobrem (`covers`), o pronta-quando é a prova
/// desses critérios, ligadas por " && " quando são mais de uma, e a ordem de
/// despacho fica no campo que a onda já reservava para isso (`order`). Cada
/// tarefa da onda ganha, na mesma passada, uma versão nova com o número dela —
/// o número velho, de spec antiga, é ignorado — e é isso que liga a onda ao
/// resto da leitura, que só conhece onda pelo `n`/`wave` gravado em cada
/// evento. A onda já entregue ou aprovada fica como história, e a tarefa dela
/// nunca volta para cá. O número da onda segue o maior já gravado, com a onda
/// desfeita e a removida incluídas: o número delas não volta a nascer.
///
/// Grava tudo junto ou nada: cada onda nova, a versão de cada tarefa e o retorno de cada tarefa desfeita são montados
/// primeiro e conferidos em sequência pela mesma conferência da gravação
/// ([`RecordCheck`]), sobre o arquivo como as anteriores o deixariam; só
/// então vão ao arquivo. Uma recusa na conferência não deixa onda gravada sem
/// as tarefas dela.
///
/// A prontidão e o agrupamento são o mesmo motor de [`crate::shared::dag`]
/// que já prova, sozinho, o desempate e o assunto de cada lote: esta função só
/// lê a spec, conta as vagas, monta a população de tarefas, pergunta ao Jev e
/// grava o que ele decidiu. Além das prontas, o motor recebe as tarefas que esperam
/// (`waiting`): a que espera só por tarefas do mesmo lote, ou já entregues, e
/// divide arquivo com ele entra no lote, depois delas. E recebe os arquivos
/// das ondas do plano ainda abertas (`busy`). `Ok(vec![])` sem vaga livre ou
/// sem tarefa pronta no backlog.
///
/// `run_round_with_mine` (`apps/rt/src/commands/flow/round/answer.rs`) chama
/// esta função antes de formar a lista de ondas prontas: quem decide as
/// ondas prontas para o pedido (`next_waves`) só lê o evento de
/// onda já gravado, e é por isso que a onda precisa existir antes de
/// `next_waves` rodar, na mesma chamada.
///
/// Recebe duas leituras da spec. `on_entry` é a de quando a rodada começou,
/// antes do relatório dela mexer em onda ou tarefa: a tarefa que o corte de
/// uma onda de lote devolve solta agora mesmo não está pronta nela, e só
/// entra na montagem da rodada seguinte, nunca na mesma que a soltou.
/// `locked` é a leitura feita já com a trava do passo do git presa, que vê o
/// que outra rodada, chegada ao mesmo tempo, gravou antes desta pegar a trava.
/// A onda leva só a tarefa pronta nas duas — a versão vigente, em `locked`,
/// da que estava pronta na entrada, ou a que estava numa onda montada e
/// desfeita agora —, e todo o resto (o número da onda nova, a versão de cada
/// tarefa, a onda mantida a atualizar) sai de `locked`: a tarefa que a outra
/// rodada já empacotou tem, ali, a onda dela, e não volta a sair numa onda com
/// o mesmo número. A tarefa que espera segue a mesma regra das duas leituras:
/// só entra a que está no backlog nas duas.
///
/// # Errors
///
/// A recusa da conferência, antes de qualquer gravação, ou a da primeira
/// gravação que falhar.
pub(crate) fn dispatch_backlog(
    start: &Path,
    spec: &str,
    on_entry: &SpecLog,
    locked: &SpecLog,
    limit: usize,
    judge: Option<&Judge<'_>>,
) -> Result<Vec<u64>, mustard_core::domain::spec_events::Refusal> {
    use crate::shared::dag::{pack_batches, Batch};

    let log = locked;
    let running = waves_in_progress(log);
    let done_waves = waves_done(log, &running);
    let by_id: BTreeMap<u64, &SpecEvent> =
        log.visible().into_iter().filter(|e| e.event_type == "task").map(|t| (t.id, t)).collect();
    let codes = log.codes();
    // A onda de lote montada e não enviada se desfaz: só existe a que roda.
    let undone = unsent_batch_waves(log);
    let in_undone = |id: &u64| by_id.get(id).and_then(|task| task.wave()).is_some_and(|wave| undone.contains(&wave));
    let cleanup = |id: &u64| by_id.get(id).is_some_and(|task| is_cleanup(task));
    // A tarefa que não cobre item nenhum não forma onda, sozinha ou de carona:
    // a onda leva os itens que as tarefas dela cobrem como critérios, e a
    // gravação recusa a que ficaria sem nenhum.
    let bare = |id: &u64| by_id.get(id).is_some_and(|task| covers_nothing(task));
    let population = backlog_population(log, &done_waves);
    // A versão vigente, em `locked`, do que a leitura de entrada via.
    let in_locked = |ids: BTreeSet<u64>| -> BTreeSet<u64> {
        ids.into_iter().filter_map(|id| log.current(id)).map(|task| task.id).collect()
    };
    let ready_on_entry = in_locked(backlog_ready(on_entry).into_iter().collect());
    let order: Vec<u64> = backlog_ready_without(log, &undone)
        .into_iter()
        .filter(|id| ready_on_entry.contains(id) || in_undone(id))
        .collect();
    let left_on_entry = in_locked(backlog_left(on_entry));
    let waiting: Vec<u64> = backlog_left_without(log, &undone)
        .into_iter()
        .filter(|id| (left_on_entry.contains(id) || in_undone(id)) && !order.contains(id) && !cleanup(id) && !bare(id))
        .collect();
    // O arquivo de cada onda do plano que ainda não terminou: em andamento,
    // por sair, mantida ou com conserto pendente.
    let graph = wave_graph(log);
    let open_waves: Vec<u64> =
        log.planned_waves().into_iter().filter(|n| !done_waves.contains(n) && !undone.contains(n)).collect();
    let busy: BTreeSet<String> =
        open_waves.iter().flat_map(|n| graph.files.get(n).cloned().unwrap_or_default()).collect();
    // As vagas que as ondas já montadas deixam livres. O ciclo entre ondas é
    // recusado pela escolha das ondas prontas, logo depois: aqui só não sobra
    // vaga para onda nova.
    let (free, alone) = outgoing(log, limit, &open_sends(log), &waves_stuck(log), &undone)
        .map_or((0, false), |out| (out.free, out.alone));

    let code_of = |id: &u64| -> u64 {
        codes.get(id).and_then(|code| code.rsplit('-').next()).and_then(|number| number.parse().ok()).unwrap_or(*id)
    };
    // Cada resumo que ainda vale vira a base de uma onda, antes de qualquer outra;
    // as tarefas dessas ondas ficam fora do resto da montagem, saiam elas
    // agora ou esperem a vez: a que saísse noutra onda perderia o resumo.
    let by_summary = summary_waves(log, &order, &population);
    let claimed: BTreeSet<u64> = by_summary.iter().flat_map(|wave| wave.batch.tasks.iter().copied()).collect();
    let order: Vec<u64> = order.into_iter().filter(|id| !claimed.contains(id)).collect();
    // O Jev julga o backlog pronto numa chamada só, e só quando há vaga para
    // uma onda nova: sem vaga ou sem tarefa pronta nada se pergunta.
    let judging = match judge {
        Some(judge) if free > 0 && !order.is_empty() => {
            let board = board_of(log, &by_id, &population, &order, &open_waves);
            let called = Instant::now();
            Some((order.len(), called, judge(&board)))
        }
        _ => None,
    };
    let batches = match &judging {
        Some((_, _, Ok(judged))) => {
            pack_by_kind(&population, &order, &waiting, &busy, &judged.tasks, &code_of, wave_budget(WAVE_LIMIT))
        }
        _ => {
            let mut batches =
                if order.is_empty() { Vec::new() } else { pack_batches(&population, &order, &waiting, &busy) };
            // O curinga da árvore inteira primeiro; os outros pela tarefa de
            // código mais baixo, e a ordem de prontidão do motor desempata.
            batches.sort_by_key(|batch| (!touches_whole_tree(&batch.files), batch.tasks.iter().map(&code_of).min()));
            batches
        }
    };
    let batches: Vec<(Batch<u64>, Option<u64>)> = by_summary
        .into_iter()
        .map(|wave| (wave.batch, Some(wave.summary)))
        .chain(batches.into_iter().map(|batch| (batch, None)))
        .collect();
    // Duas ondas com arquivo em comum nunca saem juntas: a que perde a vez
    // fica no backlog e entra na montagem da rodada em que uma vaga abrir. E
    // a que espera, por onda aberta ou por outra desta passada, reserva os
    // arquivos dela: nenhuma que vem depois, na ordem de prioridade, e divide
    // arquivo com ela sai antes.
    let mut chosen: Vec<(Batch<u64>, Option<u64>)> = Vec::new();
    let mut reserved = Reserved::default();
    for (batch, summary) in batches {
        if chosen.len() >= free {
            break;
        }
        let taken: Vec<&String> = chosen.iter().flat_map(|(picked, _)| &picked.files).collect();
        let held = sets_cross(&batch.files, &busy) || sets_cross(&batch.files, taken.iter().copied());
        if touches_whole_tree(&batch.files) {
            // O curinga da árvore inteira cruza com todos: só sai sozinho, sem
            // nada em andamento. Ele não reserva nada, senão parava todo o
            // resto atrás dele.
            if !held && alone && chosen.is_empty() {
                chosen.push((batch, summary));
                break;
            }
            continue;
        }
        // A onda pequena espera enquanto outra roda, salvo a que continua um
        // resumo. Ela reserva os arquivos como a bloqueada.
        let waits = summary.is_none() && !running.is_empty() && batch.waits_to_grow();
        if reserved.lets_out(&batch.files, held || waits) {
            chosen.push((batch, summary));
        }
    }
    if let Some((asked, called, answer)) = &judging {
        let returned = chosen.iter().filter(|(_, summary)| summary.is_none()).map(|(batch, _)| batch.tasks.len()).sum();
        record_assembly(start, spec, (*asked, returned), *called, answer);
    }

    let mut writes: Vec<(&str, Map<String, Value>)> = Vec::new();
    // O número segue o maior já gravado, com a onda que ficou vazia e a
    // removida incluídas: saíram do plano, mas o número delas não volta.
    let mut next_n = log.last_wave_number();
    let mut formed = Vec::new();
    let mut numbered: BTreeSet<u64> = BTreeSet::new();
    for (batch, summary) in &chosen {
        next_n += 1;
        let tasks: Vec<&SpecEvent> = batch.tasks.iter().filter_map(|id| by_id.get(id).copied()).collect();
        // A tarefa nascida de uma onda cobre critério, com prova para juntar
        // aqui; a que nasce do item combinado sem onda dona (o backlog da
        // revisão final) cobre o item, que não tem prova — o texto de quem
        // marcou o item sem atender é o que diz quando a onda entrega.
        let fields = mustard_core::domain::wave_prompt::backlog_fields(log, &tasks);
        let draft = json!({
            "n": next_n,
            "text": fields.text,
            "criteria": fields.criteria,
            "done_when": fields.done_when,
            "order": batch.tasks,
            "author": "binary",
        });
        let Value::Object(mut draft) = draft else { unreachable!("json! de um mapa sempre é objeto") };
        if let Some(summary) = summary {
            draft.insert("summary".into(), json!(summary));
        }
        writes.push(("wave", draft));
        for id in &batch.tasks {
            if let Some(revised) = task_revision(log, *id, Map::from_iter([("wave".to_string(), json!(next_n))])) {
                writes.push(("task", revised));
                numbered.insert(*id);
            }
        }
        formed.push(next_n);
    }
    // A tarefa da onda desfeita que não caiu em nenhum assunto montado agora
    // volta solta ao backlog.
    for task in by_id.values().filter(|task| in_undone(&task.id) && !numbered.contains(&task.id)) {
        writes.push(("task", backlog_return(task)));
    }
    if writes.is_empty() {
        return Ok(formed);
    }
    let mut check = RecordCheck::open(start, spec, PhaseWriter::Binary)?;
    for (event_type, draft) in &writes {
        check.record(event_type, draft.clone())?;
    }
    for (event_type, draft) in writes {
        record(start, spec, event_type, draft, PhaseWriter::Binary)?;
    }
    Ok(formed)
}

/// O quadro que o Jev recebe: as ondas do plano ainda abertas
/// (`open_waves`), cada uma com as tarefas e os arquivos delas, e o backlog
/// pronto (`ready`), na ordem de prontidão, com o que cada tarefa diz de si e
/// os títulos das tarefas de que depende.
fn board_of(
    log: &SpecLog,
    by_id: &BTreeMap<u64, &SpecEvent>,
    population: &[BacklogTask<u64>],
    ready: &[u64],
    open_waves: &[u64],
) -> Board {
    let seen = BoardTask::of;
    let running = open_waves
        .iter()
        .map(|n| BoardWave {
            n: *n,
            tasks: log
                .block(BlockQuery::Wave(*n))
                .into_iter()
                .filter(|e| e.event_type == "task")
                .map(|task| seen(task, Vec::new()))
                .collect(),
        })
        .collect();
    let backlog = ready
        .iter()
        .filter_map(|id| {
            let task = by_id.get(id).copied()?;
            let depends_on = population
                .iter()
                .find(|entry| entry.id == *id)
                .into_iter()
                .flat_map(|entry| entry.depends_on.iter())
                .filter_map(|dependency| by_id.get(dependency))
                .map(|dependency| dependency.str_field("title").unwrap_or_default().to_string())
                .collect();
            Some(seen(task, depends_on))
        })
        .collect();
    Board { running, backlog }
}

/// Grava o evento `call` da montagem pelo Jev, como a busca grava o dela:
/// quantas tarefas foram ao Jev e quantas saíram em onda (`asked`,
/// `returned`) e, na resposta, o tempo, os tokens, o custo e o modelo; na
/// falha, o motivo dela no nome do filtro (`jev:<motivo>`), e a montagem
/// seguiu pelo arquivo. `called` é a hora da chamada.
fn record_assembly(
    start: &Path,
    spec: &str,
    (asked, returned): (usize, usize),
    called: Instant,
    answer: &Result<Judged, FilterError>,
) {
    let mut measured = Map::new();
    measured.insert("candidates".to_string(), json!(asked));
    measured.insert("returned".to_string(), json!(returned));
    match answer {
        Ok(judged) => {
            measured.insert("filter".to_string(), json!("jev"));
            measured.insert("filter_ms".to_string(), json!(judged.usage.millis));
            measured.insert("tokens".to_string(), json!(judged.usage.input_tokens));
            measured.insert("cost_micro_usd".to_string(), json!(judged.usage.cost_micro_usd));
            measured.insert("requests".to_string(), json!(judged.usage.requests));
            if !judged.usage.model.is_empty() {
                measured.insert("model".to_string(), json!(judged.usage.model));
            }
        }
        Err(error) => {
            measured.insert("filter".to_string(), json!(format!("jev:{}", error.reason())));
            measured.insert(
                "filter_ms".to_string(),
                json!(u64::try_from(called.elapsed().as_millis()).unwrap_or(u64::MAX)),
            );
        }
    }
    let report = json!({ "ok": true, "spec": spec });
    let _ = record_measured_call(start, "wave assembly", Some(spec), None, called, &report, measured);
}

/// As ondas de lote do plano que nenhum envio despachou e nenhuma entrega
/// fechou: montadas pelo backlog e não enviadas. É de lote a onda que
/// [`dispatch_backlog`] grava, com a ordem das tarefas no campo `order` e
/// autor binário; a onda combinada à mão no plano, sem ordem, segue como está
/// até a rodada despachá-la. É a onda de lote que a rodada desfaz.
fn unsent_batch_waves(log: &SpecLog) -> BTreeSet<u64> {
    let sent: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "send")
        .filter_map(SpecEvent::wave)
        .collect();
    let delivered = log.delivered_waves();
    let formed: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "wave" && e.str_field("author") == Some("binary") && !e.ints("order").is_empty())
        .filter_map(SpecEvent::wave)
        .collect();
    log.planned_waves().into_iter().filter(|n| formed.contains(n) && !sent.contains(n) && !delivered.contains(n)).collect()
}
