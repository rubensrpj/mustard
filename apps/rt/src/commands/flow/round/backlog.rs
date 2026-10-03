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
    backlog_left, backlog_left_without, backlog_population, backlog_ready, backlog_ready_without, open_sends, outgoing,
    task_files, task_revision, waves_done, waves_in_progress, AnalysisLine,
};
use super::report::backlog_return;
use super::stops::waves_stuck;
use crate::commands::spec_events::conversation::record_measured_call;
use crate::commands::spec_events::write::{record, RecordCheck};
use crate::commands::wave::wave_overlap_check::wave_graph;
use crate::shared::dag::{pack_by_kind, sets_cross, touches_whole_tree, BacklogTask};
use crate::shared::jev::{Board, BoardTask, BoardWave, Judged};

/// Quem julga o backlog para a montagem: uma chamada só, com o quadro inteiro
/// ([`Board`]), que volta com o tipo de trabalho de cada tarefa e o quanto ela
/// pode mudar o mesmo que uma onda em andamento ([`Judged`]). A chamada que
/// falha deixa a montagem pelo arquivo, como sem o julgamento.
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
/// e o backlog pronto, e a resposta diz o tipo de trabalho de cada tarefa e o
/// quanto ela muda o mesmo que cada onda em andamento. A onda junta as
/// tarefas do mesmo tipo, tenham ou não arquivo em comum; a de tipo incerto
/// sai sozinha, e a que muda o mesmo que uma onda aberta espera no backlog.
/// As ondas saem na ordem fixa dos tipos — defeito primeiro, limpeza no fim —
/// e, dentro do tipo, pelo número ([`pack_by_kind`]). Duas ondas com arquivo
/// em comum nunca saem juntas, de tipos diferentes ou não: a que perde a vez
/// fica no backlog. A chamada grava um evento `call` com os tokens, o custo e
/// o modelo, como a busca. Sem `judge` (sem chave, ou o Jev desligado), ou com
/// a chamada falhando, o assunto é o arquivo: as tarefas que dividem arquivo,
/// direto ou por uma corrente de outras, e a de código mais baixo sai
/// primeiro ([`pack_batches`](crate::shared::dag::pack_batches)). Em qualquer
/// dos dois, a tarefa com o curinga da árvore inteira só sai sozinha, sem nada
/// em andamento.
///
/// Só existe a onda que está rodando. A onda de lote montada e não enviada de
/// uma rodada anterior — a que ficou esperando a escolha do orquestrador, por
/// exemplo — é desfeita: cada tarefa dela volta ao backlog ([`backlog_return`])
/// e entra na montagem junto das outras, e a que cai num assunto montado agora
/// ganha uma versão só, com o número da onda nova. A exceção é a onda que a
/// linha `ANALYSIS` desta rodada (`given`) respondeu: essa é mantida, ocupa
/// uma vaga e sai com a escolha que a linha trouxe.
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
/// desfeita incluída: o número dela não volta a nascer.
///
/// Na mesma chamada, [`stale_batch_revisions`] atualiza a onda mantida que
/// perdeu alguma tarefa para um evento de remoção depois de gravada — sem
/// isso o pedido dela abriria pelo `done_when` congelado na formação, citando
/// texto de tarefa que já não existe.
///
/// Grava tudo junto ou nada: as versões das ondas mantidas, cada onda nova, a
/// versão de cada tarefa e o retorno de cada tarefa desfeita são montados
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
    given: &[AnalysisLine],
    judge: Option<&Judge<'_>>,
) -> Result<Vec<u64>, mustard_core::domain::spec_events::Refusal> {
    use crate::shared::dag::{pack_batches, Batch};

    let log = locked;
    let running = waves_in_progress(log);
    let done_waves = waves_done(log, &running);
    let by_id: BTreeMap<u64, &SpecEvent> =
        log.visible().into_iter().filter(|e| e.event_type == "task").map(|t| (t.id, t)).collect();
    let codes = log.codes();
    // A onda de lote montada e não enviada só segue de pé quando a linha
    // `ANALYSIS` desta rodada a respondeu; as outras se desfazem.
    let unsent = unsent_batch_waves(log);
    let kept: BTreeSet<u64> = unsent.iter().copied().filter(|n| given.iter().any(|line| line.wave == *n)).collect();
    let undone: BTreeSet<u64> = unsent.difference(&kept).copied().collect();
    let in_undone = |id: &u64| by_id.get(id).and_then(|task| task.wave()).is_some_and(|wave| undone.contains(&wave));
    let cleanup = |id: &u64| by_id.get(id).is_some_and(|task| is_cleanup(task));
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
        .filter(|id| (left_on_entry.contains(id) || in_undone(id)) && !order.contains(id) && !cleanup(id))
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
        Some((_, _, Ok(judged))) => pack_by_kind(&population, &order, &waiting, &busy, &judged.tasks, &code_of),
        _ => {
            let mut batches =
                if order.is_empty() { Vec::new() } else { pack_batches(&population, &order, &waiting, &busy) };
            // O curinga da árvore inteira primeiro; os outros pela tarefa de
            // código mais baixo, e a ordem de prontidão do motor desempata.
            batches.sort_by_key(|batch| (!touches_whole_tree(&batch.files), batch.tasks.iter().map(&code_of).min()));
            batches
        }
    };
    // Duas ondas com arquivo em comum nunca saem juntas: a que perde a vez
    // fica no backlog e entra na montagem da rodada em que uma vaga abrir.
    let mut chosen: Vec<Batch<u64>> = Vec::new();
    for batch in batches {
        if chosen.len() >= free {
            break;
        }
        let whole_tree = touches_whole_tree(&batch.files);
        let taken: Vec<&String> = chosen.iter().flat_map(|picked| &picked.files).collect();
        if sets_cross(&batch.files, &busy)
            || sets_cross(&batch.files, taken.iter().copied())
            || (whole_tree && !(alone && chosen.is_empty()))
        {
            continue;
        }
        chosen.push(batch);
        if whole_tree {
            break;
        }
    }
    if let Some((asked, called, answer)) = &judging {
        let returned = chosen.iter().map(|batch| batch.tasks.len()).sum();
        record_assembly(start, spec, (*asked, returned), *called, answer);
    }

    let mut writes: Vec<(&str, Map<String, Value>)> =
        stale_batch_revisions(log, &kept).into_iter().map(|revised| ("wave", revised)).collect();
    // O número segue o maior já gravado, com a onda que ficou vazia incluída:
    // ela saiu do plano, mas o número dela não volta a nascer.
    let mut next_n = log.last_wave_number();
    let mut formed = Vec::new();
    let mut numbered: BTreeSet<u64> = BTreeSet::new();
    for batch in &chosen {
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
        let Value::Object(draft) = draft else { unreachable!("json! de um mapa sempre é objeto") };
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
    let seen = |task: &SpecEvent, depends_on: Vec<String>| BoardTask {
        id: task.id,
        title: task.str_field("title").unwrap_or_default().to_string(),
        text: task.str_field("text").unwrap_or_default().to_string(),
        agent: task.str_field("agent").unwrap_or_default().to_string(),
        files: task_files(task).into_iter().collect(),
        depends_on,
    };
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
/// até a rodada despachá-la. É a onda de lote que a rodada desfaz quando a
/// escolha do orquestrador não a respondeu.
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

/// A versão nova do registro de cada onda de lote mantida (`kept`: autor
/// `binary`, ainda não enviada e respondida pela linha `ANALYSIS` desta
/// rodada), quando uma tarefa dela saiu do backlog por evento de remoção
/// depois de o lote ter sido formado: sem isso o pedido abriria pelo
/// `done_when` congelado na formação, que pode citar o texto de uma tarefa
/// que não existe mais, mesmo com a lista de tarefas do pedido já saindo
/// certa. Recalcula critério, texto e pronto-quando
/// ([`mustard_core::domain::wave_prompt::backlog_fields`]) a partir das
/// tarefas que a leitura de agora mostra visíveis naquela onda — o mesmo
/// conjunto que alimenta a lista de tarefas do pedido — e monta uma versão
/// nova só quando a ordem gravada perdeu alguma tarefa; quem grava é
/// [`dispatch_backlog`], junto dos lotes novos. A onda já enviada
/// fica intocada: o pedido dela já foi montado, e mudar o registro não muda
/// o que o agente já recebeu. A onda que perdeu todas as tarefas fica de
/// fora: sem tarefa nenhuma ela saiu do plano
/// ([`SpecLog::planned_waves`]), e não há o que recalcular.
fn stale_batch_revisions(log: &SpecLog, kept: &BTreeSet<u64>) -> Vec<Map<String, Value>> {
    let mut revisions = Vec::new();
    for n in kept.iter().copied() {
        let items = log.block(BlockQuery::Wave(n));
        let Some(wave_event) = items.iter().copied().find(|e| e.event_type == "wave") else { continue };
        let tasks: Vec<&SpecEvent> = items.iter().copied().filter(|e| e.event_type == "task").collect();
        if tasks.is_empty() {
            continue;
        }
        // A ordem gravada aponta os números de antes da tarefa ganhar a
        // versão nova com o `wave` (`task_revision`, acima): segue a cadeia
        // de substituição até a versão vigente de cada uma, e só conta como
        // viva a que ainda está entre as tarefas visíveis desta onda.
        let recorded_order = wave_event.ints("order");
        let live_ids: BTreeSet<u64> = tasks.iter().map(|t| t.id).collect();
        let live_order: Vec<u64> =
            recorded_order.iter().filter_map(|id| log.current(*id)).map(|t| t.id).filter(|id| live_ids.contains(id)).collect();
        if live_order.len() == recorded_order.len() {
            continue;
        }
        let fields = mustard_core::domain::wave_prompt::backlog_fields(log, &tasks);
        let extra = Map::from_iter([
            ("text".to_string(), json!(fields.text)),
            ("criteria".to_string(), json!(fields.criteria)),
            ("done_when".to_string(), json!(fields.done_when)),
            ("order".to_string(), json!(live_order)),
        ]);
        revisions.extend(task_revision(log, wave_event.id, extra));
    }
    revisions
}

