//! A montagem das ondas a partir do backlog: a cada rodada, as ondas que saem
//! agora, no máximo uma por vaga livre, cada uma com um assunto só, e a
//! desmontagem da onda de lote que ficou montada e não saiu. Só existe a onda
//! que está rodando; o resto das tarefas fica no backlog, sem número de onda.
//! O assunto combina tipo de trabalho, arquivos e relações atuais do scan.
//! O Jev responde decisões tipadas não atendidas pelo cache; na falha,
//! o agrupamento segue pelas evidências nativas.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use mustard_core::domain::map_filter::FilterError;
use mustard_core::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use serde_json::{Map, Value, json};

use super::leftovers::is_cleanup;
use super::queue::{
    backlog_left, backlog_left_without, backlog_population, backlog_ready_without, covers_nothing, open_sends, outgoing, task_revision, waves_done,
    waves_in_progress,
};
use super::report::backlog_return;
use super::stops::waves_stuck;
use super::summary_wave::summary_waves;
use crate::commands::spec_events::write::{RecordCheck, record};
use crate::commands::wave::wave_overlap_check::wave_graph;
use crate::hooks::session::conversation_size::WAVE_LIMIT;
use crate::shared::dag::{BacklogTask, CLASH_FROM, Reserved, files_cross, pack_by_affinity, pack_by_kind, sets_cross, touches_whole_tree};
use crate::shared::jev::{ASSEMBLY_CALL, record_jev_call};
use crate::shared::judgement::{Board, BoardTask, BoardWave, Judged};

/// Quem julga o backlog para a montagem: uma avaliação do quadro inteiro
/// ([`Board`]), que volta com o tipo de trabalho de cada tarefa, o tamanho
/// dela e o quanto ela pode mudar o mesmo que uma onda em andamento
/// ([`Judged`]). A chamada que falha deixa a montagem pelo arquivo, como sem o
/// julgamento.
pub(crate) type Judge<'a> = dyn Fn(&Board) -> Result<Judged, FilterError> + 'a;

/// O que a montagem fez numa rodada: as ondas que formou e, pelo código de
/// cada tarefa pronta que ela deixou de fora, o motivo exato — o que o quadro
/// da fila, que não lê o scan nem chama o Jev, não vê. A tarefa que ficou só
/// por falta de vaga, ou pelo arquivo de uma onda aberta, não entra: o
/// quadro já diz isso dela.
pub(crate) type Assembly = (Vec<u64>, BTreeMap<String, Hold>);

/// Por que a montagem deixou de fora uma tarefa pronta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Hold {
    /// Pelo mapa do scan, ela lê o que uma onda aberta, ou formada antes
    /// dela nesta montagem, muda — ou muda o que a onda lê: a relação com
    /// cada onda, pelo número dela.
    Contract(BTreeMap<u64, Relation>),
    /// O Jev julgou que ela muda o mesmo que uma onda em andamento, com essa
    /// chance, em porcentagem.
    Judged(u8),
    /// Reservada atrás de outra tarefa que também ficou: o código dela e os
    /// arquivos que as duas dividem, vazio quando só o assunto do lote as
    /// junta.
    Behind(String, Vec<String>),
    /// Chegou ao backlog nesta rodada, devolvida pelo corte de uma onda ou
    /// nascida agora, e entra na montagem da rodada seguinte.
    Arrived,
}

/// A relação de leitura e escrita de uma tarefa com uma onda, pelo mapa do
/// scan: os arquivos que a tarefa lê e a onda muda (`reads`) e os da tarefa
/// que a onda lê (`read_by`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Relation {
    pub(crate) reads: BTreeSet<String>,
    pub(crate) read_by: BTreeSet<String>,
}

impl Relation {
    /// Junta a esta relação os arquivos da de `task` com `other`, a outra
    /// ponta, uma tarefa da onda.
    fn add(&mut self, task: &BoardTask, other: &BoardTask) {
        let crossing = |mine: &[String], theirs: &[String]| {
            mine.iter().filter(|file| theirs.iter().any(|their| files_cross(file, their))).cloned().collect::<Vec<_>>()
        };
        self.reads.extend(crossing(&task.reads, &other.files));
        self.read_by.extend(crossing(&task.files, &other.reads));
    }

    /// A relação de `task` com cada onda de `waves` cujas tarefas ela lê ou
    /// que a leem, pelo número da onda.
    fn with<'a, Tasks: IntoIterator<Item = &'a BoardTask>>(task: &BoardTask, waves: impl IntoIterator<Item = (u64, Tasks)>) -> BTreeMap<u64, Relation> {
        let mut found: BTreeMap<u64, Relation> = BTreeMap::new();
        for (n, tasks) in waves {
            for other in tasks.into_iter().filter(|other| task.relation(other).read_write) {
                found.entry(n).or_default().add(task, other);
            }
        }
        found
    }
}

/// Monta as ondas que saem nesta rodada, olhando o backlog inteiro
/// ([`backlog_left`], [`backlog_ready`]): no máximo uma por vaga livre de
/// `limit` (as vagas do projeto, `max_parallel`), cada uma com um assunto só,
/// sem corte em número de tarefas e sem juntar assuntos sem relação. O que não
/// cabe nas vagas fica no backlog, sem onda, e entra na montagem da rodada em
/// que uma vaga abrir. A limpeza ([`is_cleanup`]) só entra quando nada mais
/// resta ([`backlog_ready`]).
///
/// O assunto pode vir do Jev (`judge`), avaliando apenas decisões tipadas
/// ainda não atendidas pelo cache, quando há vaga livre e tarefa elegível.
/// O quadro leva as ondas em andamento
/// e o backlog pronto, e a resposta diz o tipo de trabalho de cada tarefa, o
/// tamanho dela e o quanto ela muda o mesmo que cada onda em andamento. A onda
/// junta as tarefas do mesmo tipo, tenham ou não arquivo em comum, até a soma
/// do tamanho delas chegar ao que um agente faz antes do limite da conversa
/// de trabalho adicional (`WAVE_LIMIT`): entrada e resumos herdados não
/// reduzem esse horizonte. O que
/// passa disso fecha a onda e vai para outra, e a tarefa que sozinha passa do
/// teto sai sozinha. A de tipo incerto sai sozinha, e a que muda o mesmo que
/// uma onda aberta espera no backlog. As ondas saem na ordem fixa dos tipos —
/// defeito primeiro, limpeza no fim — e, dentro do tipo, pelo número
/// ([`pack_by_kind`]). Duas ondas com arquivo em comum nunca saem juntas, de
/// tipos diferentes ou não: a que perde a vez fica no backlog. A chamada grava um evento `call` com os tokens, o custo e
/// o modelo, como a busca. Sem `judge` (sem chave, ou o Jev desligado), ou com
/// a chamada falhando, o assunto combina os arquivos declarados e as relações
/// atuais do scan, dentro da estimativa de tamanho. Os grupos que dividem
/// arquivo continuam atômicos ([`pack_by_local_evidence`](crate::shared::dag::pack_by_local_evidence)). Em qualquer
/// dos dois, a tarefa com o curinga da árvore inteira só sai sozinha, sem nada
/// em andamento.
///
/// A tarefa com a marca de prioridade (`priority`), que o usuário deu, sai
/// antes da ordem fixa dos tipos e do número, com o Jev e sem ele: o lote
/// dela vai na frente, e a tarefa que divide arquivo com ela espera atrás
/// dela. A marca não passa por cima da dependência, dos arquivos de uma onda
/// em andamento nem das vagas, e a limpeza segue saindo por último, marcada
/// ou não. O Jev não a julga: ela é lida do evento, e o quadro mandado a ele
/// não muda.
///
/// Uma onda pequena independente pode usar uma vaga livre. Relações de
/// leitura/escrita verificadas no scan ajudam o agrupamento e impedem que
/// consumidores concorram com contratos em mudança. Leitura compartilhada
/// sozinha não bloqueia. Arquivos reservados e dependências continuam exatos.
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
/// desses itens ([`backlog_fields`](mustard_core::domain::wave_prompt::backlog_fields)),
/// ligadas por " && " quando são mais de uma, e a ordem de
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
/// das ondas do plano ainda abertas (`busy`, [`reserving_waves`]). Sem vaga
/// livre ou sem tarefa pronta no backlog, nenhuma onda se forma.
///
/// Devolve, além das ondas formadas, o motivo exato de cada tarefa pronta que
/// ficou de fora ([`Assembly`]): a relação de leitura e escrita do scan
/// com uma onda aberta ou formada agora, o julgamento do Jev, a reserva atrás
/// de outra tarefa que também ficou, ou a chegada ao backlog nesta rodada.
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
) -> Result<Assembly, mustard_core::domain::spec_events::Refusal> {
    use crate::shared::dag::{Batch, pack_by_local_evidence};

    let log = locked;
    let running = waves_in_progress(log);
    let done_waves = waves_done(log, &running);
    let by_id: BTreeMap<u64, &SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "task").map(|t| (t.id, t)).collect();
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
    // As tarefas com a marca de prioridade, lidas do evento: o lote delas
    // sai antes dos outros e não espera juntar trabalho.
    let marked: BTreeSet<u64> = population.iter().filter(|task| task.priority).map(|task| task.id).collect();
    // A versão vigente, em `locked`, do que a leitura de entrada via.
    let in_locked = |ids: BTreeSet<u64>| -> BTreeSet<u64> {
        ids.into_iter()
            .filter_map(|id| log.current(id).filter(|task| task.id == id || task.str_field("author") != Some("binary")))
            .map(|task| task.id)
            .collect()
    };
    let left_on_entry = in_locked(backlog_left(on_entry));
    // A pronta agora que não estava no backlog na entrada — devolvida pelo
    // corte de uma onda nesta rodada, ou nascida nela — fica para a seguinte.
    let (order, arrived): (Vec<u64>, Vec<u64>) =
        backlog_ready_without(log, &undone).into_iter().partition(|id| left_on_entry.contains(id) || in_undone(id));
    let waiting: Vec<u64> = backlog_left_without(log, &undone)
        .into_iter()
        .filter(|id| (left_on_entry.contains(id) || in_undone(id)) && !order.contains(id) && !cleanup(id) && !bare(id))
        .filter(|id| !population.iter().any(|task| task.id == *id && task.blocked))
        .collect();
    // O arquivo de cada onda do plano que ainda não terminou: em andamento,
    // por sair, mantida ou com conserto pendente.
    let graph = wave_graph(log);
    let open_waves = reserving_waves(log);
    let busy: BTreeSet<String> = open_waves.iter().flat_map(|n| graph.files.get(n).cloned().unwrap_or_default()).collect();
    // As vagas que as ondas já montadas deixam livres. O ciclo entre ondas é
    // recusado pela escolha das ondas prontas, logo depois: aqui só não sobra
    // vaga para onda nova.
    let (free, alone) = outgoing(log, limit, &open_sends(log), &waves_stuck(log), &undone).map_or((0, false), |out| (out.free, out.alone));

    let code_of = |id: &u64| -> u64 { codes.get(id).and_then(|code| code.rsplit('-').next()).and_then(|number| number.parse().ok()).unwrap_or(*id) };
    // Cada resumo que ainda vale vira a base de uma onda, antes de qualquer outra;
    // as tarefas dessas ondas ficam fora do resto da montagem, saiam elas
    // agora ou esperem a vez: a que saísse noutra onda perderia o resumo.
    let by_summary = summary_waves(log, &order, &population);
    let claimed: BTreeSet<u64> = by_summary.iter().flat_map(|wave| wave.batch.tasks.iter().copied()).collect();
    let order: Vec<u64> = order.into_iter().filter(|id| !claimed.contains(id)).collect();
    let tidy: Vec<_> = crate::shared::dag::ready_tasks(&population).into_iter()
        .filter(|id| left_on_entry.contains(id) && cleanup(id) && !bare(id)).collect();
    let candidates: Vec<_> = order.iter().chain(&waiting).chain(&claimed).chain(&tidy).copied().collect();
    let mut affinity_board = board_of(log, &by_id, &population, &candidates, &open_waves);
    affinity_board.with_expertise(start);
    if free > 0 && (!order.is_empty() || !claimed.is_empty()) {super::evidence::augment(start, &mut affinity_board);}
    // A consumer cannot read a contract another running wave is changing.
    // Shared reading remains harmless. The actual source verified above,
    // rather than semantic similarity, establishes this reservation.
    let opened = busy.clone();
    let mut busy = busy;
    let mut contracts: BTreeMap<u64, BTreeMap<u64, Relation>> = BTreeMap::new();
    for task in &affinity_board.backlog {
        let found = Relation::with(task, affinity_board.running.iter().map(|wave| (wave.n, &wave.tasks)));
        if !found.is_empty() {
            busy.extend(task.files.iter().cloned());
            contracts.insert(task.id, found);
        }
    }
    // Only uncached typed decisions are delegated when a slot is available.
    // Native source relationships are reused in the paid board.
    let eligible = order.iter().filter_map(|id| population.iter().find(|task| task.id == *id))
        .any(|task| !sets_cross(&task.files, &busy) && (!touches_whole_tree(&task.files) || alone));
    let judging = match judge {
        Some(judge) if free > 0 && eligible => {
            // Only dependent candidates that can become eligible in this
            // batch need a profile. Their intrinsic cache key is independent
            // of the running board, just like already-ready tasks.
            let mut candidates: BTreeSet<u64> = order.iter().copied().collect();
            let done: BTreeSet<u64> = population.iter().filter(|t| t.done).map(|t| t.id).collect();
            loop {
                let before = candidates.len();
                for task in population.iter().filter(|t| waiting.contains(&t.id) && !sets_cross(&t.files, &busy)) {
                    if task.depends_on.iter().all(|d| done.contains(d) || candidates.contains(d)) {
                        candidates.insert(task.id);
                    }
                }
                if before == candidates.len() {
                    break;
                }
            }
            let mut board = board_of(log, &by_id, &population, &candidates.into_iter().collect::<Vec<_>>(), &open_waves);
            board.with_expertise(start);
            for task in &mut board.backlog {
                if let Some(known) = affinity_board.backlog.iter().find(|known| known.id == task.id) {task.reads.clone_from(&known.reads);}
            }
            board.running.clone_from(&affinity_board.running);
            let called = Instant::now();
            Some((board.backlog.len(), called, judge(&board)))
        }
        _ => None,
    };
    let affinity = affinity_board.affinities();
    let batches = match &judging {
        Some((_, _, Ok(judged))) if affinity.is_empty() => pack_by_kind(&population, &order, &waiting, &busy, &judged.tasks, &code_of, WAVE_LIMIT),
        Some((_, _, Ok(judged))) => pack_by_affinity(&population, &order, &waiting, &busy, &judged.tasks, &code_of, (WAVE_LIMIT, &affinity)),
        _ => {
            let mut batches = if order.is_empty() { Vec::new() } else {
                pack_by_local_evidence(&population, &order, &waiting, &busy, &super::evidence::links(&affinity_board), WAVE_LIMIT)
            };
            // O lote com a tarefa marcada como prioridade primeiro; depois o
            // curinga da árvore inteira; os outros pela tarefa de código mais
            // baixo, e a ordem de prontidão do motor desempata.
            batches.sort_by_key(|batch| (!batch.holds_any(&marked), !touches_whole_tree(&batch.files), batch.tasks.iter().map(&code_of).min()));
            batches
        }
    };
    let batches: Vec<(Batch<u64>, Option<u64>)> =
        by_summary.into_iter().map(|wave| (wave.batch, Some(wave.summary))).chain(batches.into_iter().map(|batch| (batch, None))).collect();
    // Duas ondas com arquivo em comum nunca saem juntas: a que perde a vez
    // fica no backlog e entra na montagem da rodada em que uma vaga abrir. E
    // a que espera, por onda aberta ou por outra desta passada, reserva os
    // arquivos dela: nenhuma que vem depois, na ordem de prioridade, e divide
    // arquivo com ela sai antes.
    let lot: BTreeMap<u64, usize> = batches.iter().enumerate().flat_map(|(at, (batch, _))| batch.tasks.iter().map(move |id| (*id, at))).collect();
    let on_board = |id: &u64| affinity_board.backlog.iter().find(|task| task.id == *id);
    // A relação com uma onda formada antes nesta passada: ela ganha o número
    // seguinte ao maior gravado, na ordem em que sai.
    let mut with_formed: BTreeMap<u64, BTreeMap<u64, Relation>> = BTreeMap::new();
    let mut chosen: Vec<(Batch<u64>, Option<u64>)> = Vec::new();
    let mut reserved = Reserved::default();
    for (batch, summary) in batches {
        if chosen.len() >= free {
            break;
        }
        let taken: Vec<&String> = chosen.iter().flat_map(|(picked, _)| &picked.files).collect();
        let picked = || chosen.iter().zip(log.last_wave_number() + 1..).map(|((picked, _), n)| (n, picked.tasks.iter().filter_map(on_board)));
        let clashing: BTreeMap<u64, BTreeMap<u64, Relation>> = batch
            .tasks
            .iter()
            .filter_map(on_board)
            .map(|task| (task.id, Relation::with(task, picked())))
            .filter(|(_, found)| !found.is_empty())
            .collect();
        let clash = !clashing.is_empty();
        let held = clash || sets_cross(&batch.files, &busy) || sets_cross(&batch.files, taken.iter().copied());
        if clash {
            with_formed.extend(clashing);
        }
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
        // Small independent batches do not wait for a file-count threshold.
        if reserved.lets_out(&batch.files, held) {
            chosen.push((batch, summary));
        }
    }
    // A chamada da montagem fica gravada, com quantas tarefas foram ao Jev e
    // quantas saíram em onda; na falha, com o motivo, e a montagem seguiu
    // pelo arquivo.
    if let Some((asked, called, answer)) = &judging {
        let returned = chosen.iter().filter(|(_, summary)| summary.is_none()).map(|(batch, _)| batch.tasks.len()).sum();
        record_jev_call(start, spec, ASSEMBLY_CALL, *called, Some((*asked, returned)), answer.as_ref().map(|judged| &judged.usage));
    }

    // Attach a ready, small cleanup to the last functional batch when local
    // source evidence links it. Never add unrelated work or reopen active waves.
    let complete = open_waves.is_empty() && left_on_entry.iter().filter(|id| !cleanup(id))
        .all(|id| chosen.iter().any(|(batch, _)| batch.tasks.contains(id)));
    if complete {
        let links = super::evidence::links(&affinity_board);
        let weight = |id: &u64| {
            let native = crate::shared::task_size::native_growth(population.iter().find(|task| task.id == *id).map_or(0, |task| task.files.len()));
            match &judging { Some((_, _, Ok(judged))) => judged.tasks.get(id).map_or(native, |size| native.max(size.growth_tokens())), _ => native }
        };
        for id in tidy {
            let Some(task) = population.iter().find(|task| task.id == id && task.files.len() == 1) else { continue; };
            if chosen.iter().any(|(batch, _)| batch.tasks.contains(&id)) || sets_cross(&task.files, &busy) { continue; }
            let target = chosen.iter().position(|(batch, _)| !touches_whole_tree(&batch.files)
                && (sets_cross(&task.files, &batch.files) || batch.tasks.iter().any(|other| links.contains(&(id,*other)) || links.contains(&(*other,id))))
                && batch.tasks.iter().map(&weight).sum::<u64>().saturating_add(weight(&id)) <= WAVE_LIMIT);
            let Some(at) = target else { continue; };
            if chosen.iter().enumerate().any(|(other,(batch,_))| other != at && sets_cross(&task.files, &batch.files)) { continue; }
            chosen[at].0.tasks.push(id);
            chosen[at].0.files.extend(task.files.iter().cloned());
        }
    }
    let judged = match &judging {
        Some((_, _, Ok(judged))) => Some(&judged.tasks),
        _ => None,
    };
    let assembled = Assembled { population: &population, codes: &codes, claimed: &claimed, order: &order, chosen: &chosen, lot: &lot, opened: &opened };
    let held = why_held(&assembled, Reasons { contracts, with_formed, judged, arrived });

    let mut writes: Vec<(&str, Map<String, Value>)> = Vec::new();
    // Os comandos que o projeto declara: o de teste reconhece a prova que o
    // relato de um pedido cita, e o de compilação prova o que não cita
    // nenhuma.
    let commands = mustard_core::ProjectConfig::load(&crate::commands::spec_events::project(start).root).commands();
    // O número segue o maior já gravado, com a onda que ficou vazia e a
    // removida incluídas: saíram do plano, mas o número delas não volta.
    let mut next_n = log.last_wave_number();
    let mut formed = Vec::new();
    let mut numbered: BTreeSet<u64> = BTreeSet::new();
    for (batch, summary) in &chosen {
        next_n += 1;
        let tasks: Vec<&SpecEvent> = batch.tasks.iter().filter_map(|id| by_id.get(id).copied()).collect();
        // A tarefa do plano cobre critério, com prova para juntar aqui; a
        // que nasce de uma sobra cobre o pedido dela, pronto pela prova que
        // ela nomeia ou pela compilação; a que nasce do item combinado sem
        // onda dona (o backlog da revisão final) cobre o item, que não tem
        // prova — o texto de quem marcou o item sem atender é o que diz
        // quando a onda entrega.
        let fields = mustard_core::domain::wave_prompt::backlog_fields(log, &tasks, &commands);
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
        return Ok((formed, held));
    }
    let mut check = RecordCheck::open(start, spec, PhaseWriter::Binary)?;
    for (event_type, draft) in &writes {
        check.record(event_type, draft.clone())?;
    }
    for (event_type, draft) in writes {
        record(start, spec, event_type, draft, PhaseWriter::Binary)?;
    }
    Ok((formed, held))
}

/// O que a montagem decidiu, para dizer por que cada tarefa pronta ficou: a
/// população do backlog, o código de cada tarefa, as presas a um resumo
/// (`claimed`) e as prontas (`order`), na ordem de prioridade, as ondas que
/// saem (`chosen`), o lote de cada tarefa (`lot`) e os arquivos das ondas
/// abertas (`opened`).
struct Assembled<'a> {
    population: &'a [BacklogTask<u64>],
    codes: &'a BTreeMap<u64, String>,
    claimed: &'a BTreeSet<u64>,
    order: &'a [u64],
    chosen: &'a [(crate::shared::dag::Batch<u64>, Option<u64>)],
    lot: &'a BTreeMap<u64, usize>,
    opened: &'a BTreeSet<String>,
}

/// Os motivos próprios que a montagem viu: a relação do scan com uma onda
/// aberta (`contracts`) e com uma formada antes nesta passada
/// (`with_formed`), o julgamento do Jev (`judged`) e as tarefas que chegaram
/// ao backlog nesta rodada (`arrived`).
struct Reasons<'a> {
    contracts: BTreeMap<u64, BTreeMap<u64, Relation>>,
    with_formed: BTreeMap<u64, BTreeMap<u64, Relation>>,
    judged: Option<&'a BTreeMap<u64, crate::shared::dag::Judgement>>,
    arrived: Vec<u64>,
}

/// O motivo de cada tarefa pronta que não saiu, pelo código dela. Vem antes o
/// motivo dela mesma: a relação do scan com uma onda aberta, o Jev, a
/// relação com uma onda formada agora. Sem nenhum, ela está reservada atrás
/// da primeira, na ordem de prioridade, que também ficou — por motivo
/// próprio, pelo arquivo de uma onda aberta ou atrás de outra — e divide
/// arquivo ou lote com ela. A que ficou só pela vaga, ou só pelo arquivo de
/// uma onda aberta, não ganha motivo aqui: o quadro da fila já o diz.
fn why_held(at: &Assembled<'_>, reasons: Reasons<'_>) -> BTreeMap<String, Hold> {
    let Reasons { contracts, mut with_formed, judged, arrived } = reasons;
    let out: BTreeSet<u64> = at.chosen.iter().flat_map(|(batch, _)| batch.tasks.iter().copied()).collect();
    let looked: Vec<u64> = at.claimed.iter().chain(at.order).copied().filter(|id| !out.contains(id)).collect();
    let mut held: BTreeMap<u64, Hold> = arrived.into_iter().map(|id| (id, Hold::Arrived)).collect();
    for id in &looked {
        let chance = judged.and_then(|judged| judged.get(id)).map(|judgement| judgement.clash).filter(|clash| *clash >= CLASH_FROM);
        let hold = match (contracts.get(id), chance) {
            (Some(waves), _) => Hold::Contract(waves.clone()),
            (None, Some(clash)) => Hold::Judged((clash * 100.0).round() as u8),
            (None, None) => match with_formed.remove(id) {
                Some(waves) => Hold::Contract(waves),
                None => continue,
            },
        };
        held.insert(*id, hold);
    }
    let files = |id: &u64| at.population.iter().find(|task| task.id == *id).map(|task| &task.files);
    let code = |id: &u64| at.codes.get(id).cloned().unwrap_or_else(|| id.to_string());
    loop {
        let behind: Vec<(u64, Hold)> = looked
            .iter()
            .filter(|id| !held.contains_key(id))
            .filter_map(|id| {
                let mine = files(id).filter(|mine| !sets_cross(*mine, at.opened))?;
                looked.iter().filter(|other| *other != id).find_map(|other| {
                    let theirs = files(other)?;
                    let waits = held.contains_key(other) || sets_cross(theirs, at.opened);
                    let shared: Vec<String> = mine.iter().filter(|file| theirs.iter().any(|their| files_cross(file, their))).cloned().collect();
                    let together = at.lot.get(id).is_some_and(|lot| at.lot.get(other) == Some(lot));
                    (waits && (together || !shared.is_empty())).then(|| (*id, Hold::Behind(code(other), shared)))
                })
            })
            .collect();
        if behind.is_empty() {
            return held.into_iter().map(|(id, hold)| (code(&id), hold)).collect();
        }
        held.extend(behind);
    }
}

/// O quadro que o Jev recebe: as ondas do plano ainda abertas
/// (`open_waves`), cada uma com as tarefas e os arquivos delas, e o backlog
/// pronto (`ready`), na ordem de prontidão, com o que cada tarefa diz de si e
/// os títulos das tarefas de que depende.
fn board_of(log: &SpecLog, by_id: &BTreeMap<u64, &SpecEvent>, population: &[BacklogTask<u64>], ready: &[u64], open_waves: &[u64]) -> Board {
    let seen = BoardTask::of;
    let running = open_waves
        .iter()
        .map(|n| BoardWave {
            n: *n,
            tasks: log.block(BlockQuery::Wave(*n)).into_iter().filter(|e| e.event_type == "task").map(|task| seen(task, Vec::new())).collect(),
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

/// As ondas do plano que reservam os arquivos delas contra o backlog: as que
/// ainda não terminaram — em andamento, por sair, mantidas ou com conserto
/// pendente —, fora a onda de lote montada e não enviada, que a montagem
/// desfaz. A montagem não solta tarefa que cruze arquivo delas, e o quadro da
/// fila diz a onda que segura cada uma.
pub(super) fn reserving_waves(log: &SpecLog) -> Vec<u64> {
    let done = waves_done(log, &waves_in_progress(log));
    let undone = unsent_batch_waves(log);
    log.planned_waves().into_iter().filter(|n| !done.contains(n) && !undone.contains(n)).collect()
}

/// As ondas de lote do plano que nenhum envio despachou e nenhuma entrega
/// fechou: montadas pelo backlog e não enviadas. É de lote a onda que
/// [`dispatch_backlog`] grava, com a ordem das tarefas no campo `order` e
/// autor binário; a onda combinada à mão no plano, sem ordem, segue como está
/// até a rodada despachá-la. É a onda de lote que a rodada desfaz.
fn unsent_batch_waves(log: &SpecLog) -> BTreeSet<u64> {
    let failure=log.visible().into_iter().filter(|e|e.event_type=="call" && e.str_field("command")==Some("round") && e.str_field("result")==Some("refused"))
        .map(|e|e.id).next_back();
    let revised=log.visible().into_iter().filter(|e|e.str_field("author")!=Some("binary") && ["task","request","criterion","wave"].contains(&e.event_type.as_str()))
        .map(|e|e.id).max().unwrap_or(0);
    let sent: BTreeSet<u64> = log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "send").filter_map(SpecEvent::wave).collect();
    let delivered = log.delivered_waves();
    let formed: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "wave" && e.str_field("author") == Some("binary") && !e.ints("order").is_empty())
        .filter_map(SpecEvent::wave)
        .collect();
    log.planned_waves().into_iter().filter(|n| {
        let recoverable=failure.is_some_and(|failed| failed>revised && log.visible().into_iter().any(|e|e.event_type=="wave" && e.wave()==Some(*n) && e.id<failed));
        formed.contains(n) && !sent.contains(n) && !delivered.contains(n) && !recoverable
    }).collect()
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::spec_events::SpecLog;
    use mustard_core::io::spec_events as store;
    use serde_json::json;
    use tempfile::tempdir;

    use super::super::tests::{approved, git_at, returned, round, waves_in};

    /// A spec `x` como está gravada agora.
    fn spec_x(root: &std::path::Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// Num projeto TypeScript, a onda 1, com critério de prova própria, volta
    /// com uma sobra que nomeia a prova dela. A onda de conserto que a rodada
    /// seguinte monta do backlog leva como critério só o pedido da sobra, e
    /// fica pronta quando passa a prova que a sobra nomeia, não a do
    /// critério da onda 1.
    #[test]
    fn a_wave_from_a_leftover_task_is_done_when_its_own_proof_passes() {
        // A onda de conserto nasce do mapa que o scan grava: sem o scan
        // compilado junto, ela não se forma, e o teste passaria à toa.
        assert!(
            mustard_core::Scan::locate().is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.ts"], &[])]);
        std::fs::write(root.join("mustard.json"), json!({"buildCommand": "echo build", "testCommand": "npm test"}).to_string()).unwrap();
        std::fs::write(root.join("src/soma.ts"), "export function soma(a: number, b: number): number { return a + b; }\n").unwrap();
        git_at(root, &["checkout", "-q", "-b", "feature/x"]);
        round(root, "x", None);

        std::fs::write(root.join("src/a.ts"), "export const one = 1;\n").unwrap();
        let wrote = returned(
            root,
            json!({"wave": 1, "text": "Saiu.", "files": ["src/a.ts"], "commit": "sai",
                "leftovers": [{"title": "A soma perde o sinal", "files": ["src/soma.ts"],
                    "detail": "Com número negativo, a soma perde o sinal. Prova: `npm test -- soma`."}]}),
        );
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let request = out["recorded"].as_array().unwrap().iter().find(|r| r["type"] == json!("request")).and_then(|r| r["id"].as_u64());
        let request = request.unwrap_or_else(|| panic!("a sobra não virou pedido: {out}"));

        let next = round(root, "x", None);
        assert_eq!(waves_in(&next, "dispatch"), vec![2], "a onda do conserto sai: {next}");
        let log = spec_x(root);
        let fix = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(2)).unwrap();
        assert_eq!(fix.fields["criteria"], json!([request]), "nenhum critério alheio: {:?}", fix.fields);
        assert_eq!(fix.fields["done_when"], json!("npm test -- soma"), "{:?}", fix.fields);
    }
}
