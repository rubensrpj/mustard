//! A montagem das ondas a partir do backlog: a cada rodada, as ondas que saem
//! agora, no máximo uma por vaga livre, cada uma com um assunto só, e a
//! desmontagem da onda de lote que ficou montada e não saiu. Só existe a onda
//! que está rodando; o resto das tarefas fica no backlog, sem número de onda.
//! O assunto combina tipo de trabalho, arquivos e relações atuais do scan.
//! Nenhuma tarefa espera por arquivo: cada onda trabalha na própria cópia, e
//! a rodada junta no commit. Seguram a tarefa só a dependência declarada, a
//! limpeza por último e as vagas.
//! O Jev responde decisões tipadas não atendidas pelo cache, antes das
//! travas da rodada ([`super::dispatch_jev`]): com elas presas, a montagem só
//! lê a decisão gravada, e a tarefa sem decisão espera a rodada seguinte, só
//! ela. O Jev que falha manda a pergunta à reserva, e a tarefa espera o
//! recibo; o recibo incerto, como o Jev desligado, deixa o agrupamento pelas
//! evidências nativas.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use mustard_core::domain::map_filter::{FilterError, FilterUsage};
use mustard_core::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use serde_json::{Map, Value, json};

use super::leftovers::is_cleanup;
use super::queue::{
    backlog_left, backlog_left_without, backlog_population, backlog_ready_without, covers_nothing, outgoing, task_revision,
    tasks_not_delivered, waves_done, waves_in_progress,
};
use super::report::backlog_return;
use super::stops::waves_stuck;
use super::summary_wave::{group_waves, summary_waves};
use crate::commands::spec_events::write::{RecordCheck, record};
use crate::shared::dag::{BacklogTask, CLASH_FROM, pack_by_kind, sets_cross, touches_whole_tree};
use super::dispatch_jev::{Asker, unsure};
use crate::shared::jev::{ASSEMBLY_CALL, backlog_payloads, read_judgements, record_jev_call};
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
/// por falta de vaga não entra: o quadro já diz isso dela.
pub(crate) type Assembly = (Vec<u64>, BTreeMap<String, Hold>);

/// Por que a montagem deixou de fora uma tarefa pronta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Hold {
    /// O Jev julgou que ela muda o mesmo que uma onda em andamento, com essa
    /// chance, em porcentagem.
    Judged(u8),
    /// Chegou ao backlog nesta rodada, devolvida pelo corte de uma onda ou
    /// nascida agora, e entra na montagem da rodada seguinte.
    Arrived,
    /// Espera a decisão do Jev, ou o recibo da reserva dele, que a passada
    /// antes das travas da rodada seguinte lê.
    JevPending,
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
/// e o backlog pronto, e a resposta diz o tipo de trabalho de cada tarefa e o
/// quanto ela muda o mesmo que cada onda em andamento. A onda junta todas as
/// tarefas do mesmo tipo, tenham ou não arquivo em comum, sem teto de tarefas
/// nem de tamanho: quem segura o tamanho é o sinal de conclusão dentro da
/// conversa do agente, que o manda terminar a tarefa atual e devolver o
/// resumo. A de tipo incerto sai sozinha, e a que o Jev julga mudar
/// o mesmo que uma onda aberta espera no backlog. As ondas saem na ordem fixa
/// dos tipos — defeito primeiro, limpeza no fim — e, dentro do tipo, pelo
/// número ([`pack_by_kind`]). Duas ondas com arquivo em comum saem juntas, e
/// a que divide arquivo com uma onda em andamento também sai: cada onda
/// trabalha na própria cópia, e a rodada junta no commit. A chamada grava um evento `call` com os tokens, o custo e
/// o modelo, como a busca. Sem `judge` (sem chave, ou o Jev desligado), ou com
/// a chamada falhando, o assunto combina os arquivos declarados e as relações
/// atuais do scan: o grupo que divide arquivo sai inteiro numa onda, com a
/// tarefa que espera só por ele ([`pack_by_local_evidence`](crate::shared::dag::pack_by_local_evidence)). Em qualquer
/// dos dois, a tarefa com o curinga da árvore inteira só sai sozinha, sem nada
/// em andamento.
///
/// A tarefa com a marca de prioridade (`priority`), que o usuário deu, sai
/// antes da ordem fixa dos tipos e do número, com o Jev e sem ele: o lote
/// dela vai na frente, e a tarefa que divide arquivo com ela vai atrás dela.
/// A marca não passa por cima da dependência nem das vagas, e a limpeza
/// segue saindo por último, marcada ou não; o arquivo de uma onda em
/// andamento não a segura, como não segura tarefa nenhuma. O Jev não a
/// julga: ela é lida do evento, e o quadro mandado a ele não muda.
///
/// Uma onda pequena independente pode usar uma vaga livre. Relações de
/// leitura/escrita verificadas no scan só ajudam o agrupamento: o consumidor
/// que lê o contrato que uma onda em andamento muda sai mesmo assim. As
/// dependências continuam exatas.
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
/// versão só, com o número da onda nova. A que a rodada segurou à espera da
/// escolha dos itens pelo Jev não se desfaz
/// ([`held_for_the_jev`](super::item_choice::held_for_the_jev)): segue
/// gravada, com as tarefas dela, e sai com o número que ganhou na rodada em
/// que a decisão chega, sem deixar onda vazia para trás.
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
/// divide arquivo com ele entra no lote, depois delas. As ondas do plano ainda
/// abertas ([`open_plan_waves`]) vão ao Jev como as ondas em andamento e não
/// reservam arquivo. Sem vaga livre ou sem tarefa pronta no backlog, nenhuma
/// onda se forma.
///
/// Devolve, além das ondas formadas, o motivo exato de cada tarefa pronta que
/// ficou de fora ([`Assembly`]): o julgamento do Jev ou a chegada ao backlog
/// nesta rodada.
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
    (judge, asker): (Option<&Judge<'_>>, Option<&Asker<'_>>),
) -> Result<Assembly, mustard_core::domain::spec_events::Refusal> {
    assemble(start, spec, (on_entry, locked), limit, (judge, asker), (false, false)).map(|(assembly, _)| assembly)
}

/// A montagem que a rodada faria agora na spec lida em `log`, sem gravar
/// nada: a spec como as gravações dela a deixariam. É a passada antes das
/// travas, com as perguntas ao Jev feitas e gravadas por `asker`; com
/// `formed_only`, só as ondas do classificador.
///
/// # Errors
///
/// A recusa da conferência das gravações.
pub(crate) fn planned_backlog(
    start: &Path,
    spec: &str,
    log: &SpecLog,
    limit: usize,
    (judge, asker): (Option<&Judge<'_>>, &Asker<'_>),
    formed_only: bool,
) -> Result<SpecLog, mustard_core::domain::spec_events::Refusal> {
    let (_, planned) = assemble(start, spec, (log, log), limit, (judge, Some(asker)), (formed_only, true))?;
    Ok(planned.unwrap_or_else(|| log.clone()))
}

/// [`dispatch_backlog`] com a formação do classificador pendente: só as
/// ondas que ele já formou saem, inteiras; as tarefas que ele ainda vai ler
/// esperam a leitura, sem Jev e sem a montagem de sempre.
pub(crate) fn dispatch_formed(start: &Path, spec: &str, on_entry: &SpecLog, locked: &SpecLog, limit: usize) -> Result<Assembly, mustard_core::domain::spec_events::Refusal> {
    assemble(start, spec, (on_entry, locked), limit, (None, None), (true, false)).map(|(assembly, _)| assembly)
}

/// O julgamento do quadro de uma montagem: as tarefas que foram ao Jev, o
/// julgamento das que têm decisão (`None` deixa a montagem pelas evidências
/// nativas), as que esperam a decisão e a chamada que saiu agora, com o uso
/// ou a falha.
struct Judging {
    asked: usize,
    judged: Option<Judged>,
    undecided: BTreeSet<u64>,
    call: Option<(Instant, Result<FilterUsage, FilterError>)>,
}

/// As tarefas do backlog que o pedido `payload` da montagem pergunta.
fn tasks_of(payload: &str) -> Vec<u64> {
    let body: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
    body["state"]["backlog"].as_object().into_iter().flatten().filter_map(|(key, _)| key.strip_prefix('t')?.parse().ok()).collect()
}

/// A resposta, na forma da do Jev, ao pedido `payload` da montagem, pelo que
/// `judged` diz de cada tarefa dele; `None` quando falta alguma.
fn board_doc(payload: &str, judged: &Judged) -> Option<Value> {
    let body: Value = serde_json::from_str(payload).ok()?;
    let mut answers = Map::new();
    for key in body["questions"].as_object()?.keys() {
        let id: u64 = key.rsplit("_t").next()?.parse().ok()?;
        let judgement = judged.tasks.get(&id)?;
        let answer = if key.starts_with("tipo_") {
            let kind = judgement.kind.key();
            json!({ "type": "choice", "choice": kind, "confidence": judgement.confidence, "probabilities": { kind: judgement.confidence } })
        } else {
            json!({ "type": "noul", "noul": judgement.clash })
        };
        answers.insert(key.clone(), answer);
    }
    Some(json!({ "answers": answers }))
}

/// O julgamento do quadro `board` por `judge`, decidido por `asker`: antes
/// das travas, a chamada sai para os pedidos sem decisão e a resposta fica
/// gravada; com as travas presas, só se lê. A tarefa de um pedido sem decisão
/// espera; a decisão incerta da reserva, ou a regra gravada, deixa a montagem
/// pelas evidências nativas. Sem `asker`, a chamada sai na hora, inteira.
fn judged_board(board: &Board, judge: &Judge<'_>, asker: Option<&Asker<'_>>) -> Judging {
    let asked = board.backlog.len();
    let Some(asker) = asker else {
        let called = Instant::now();
        let answer = judge(board);
        return Judging { asked, judged: answer.as_ref().ok().cloned(), undecided: BTreeSet::new(), call: Some((called, answer.map(|judged| judged.usage))) };
    };
    let payloads = match backlog_payloads(board) {
        Ok(payloads) => payloads,
        Err(error) => return Judging { asked, judged: None, undecided: BTreeSet::new(), call: asker.asks().then(|| (Instant::now(), Err(error))) },
    };
    let mut call = None;
    if asker.asks() && !asker.missing(ASSEMBLY_CALL, &payloads).is_empty() && !asker.take_receipt(ASSEMBLY_CALL, &payloads) {
        let wanted = asker.missing(ASSEMBLY_CALL, &payloads);
        let called = Instant::now();
        let answer = judge(board);
        let docs = answer.as_ref().ok().and_then(|judged| wanted.iter().map(|payload| board_doc(payload, judged)).collect::<Option<Vec<_>>>());
        asker.settle(ASSEMBLY_CALL, &wanted, docs, true, called);
        call = Some((called, answer.map(|judged| judged.usage)));
    }
    let known = asker.lookup(ASSEMBLY_CALL, &payloads);
    let docs: Vec<Value> = known.iter().flatten().cloned().collect();
    if docs.iter().any(Value::is_null) || unsure(&docs) {
        return Judging { asked, judged: None, undecided: BTreeSet::new(), call };
    }
    let undecided: BTreeSet<u64> = payloads.iter().zip(&known).filter(|(_, doc)| doc.is_none()).flat_map(|(payload, _)| tasks_of(payload)).collect();
    let decided =
        Board { work: board.work.clone(), running: board.running.clone(), backlog: board.backlog.iter().filter(|task| !undecided.contains(&task.id)).cloned().collect() };
    let mut answers = Map::new();
    for doc in &docs {
        answers.extend(doc["answers"].as_object().cloned().unwrap_or_default());
    }
    match read_judgements(&json!({ "answers": answers }), &decided) {
        Ok(tasks) => Judging { asked, judged: Some(Judged { tasks, usage: FilterUsage::default() }), undecided, call },
        Err(_) => Judging { asked, judged: None, undecided: BTreeSet::new(), call },
    }
}

/// O corpo de [`dispatch_backlog`], de [`dispatch_formed`] e de
/// [`planned_backlog`]: com `formed_only`, só as ondas do classificador
/// saem; com `dry`, nada se grava, e volta também a spec como as gravações a
/// deixariam.
fn assemble(
    start: &Path,
    spec: &str,
    (on_entry, locked): (&SpecLog, &SpecLog),
    limit: usize,
    (judge, asker): (Option<&Judge<'_>>, Option<&Asker<'_>>),
    (formed_only, dry): (bool, bool),
) -> Result<(Assembly, Option<SpecLog>), mustard_core::domain::spec_events::Refusal> {
    use crate::shared::dag::{Batch, pack_by_local_evidence};

    let log = locked;
    let running = waves_in_progress(log);
    let done_waves = waves_done(log, &running);
    let by_id: BTreeMap<u64, &SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "task").map(|t| (t.id, t)).collect();
    let codes = log.codes();
    // A onda de lote montada e não enviada se desfaz: só existe a que roda.
    // A recusa da rodada que a segura é das chamadas que o programa observou.
    let observed = crate::commands::spec_events::conversation::observed(start, spec, log);
    let undone = unsent_batch_waves(log, &observed);
    // A que a rodada segurou à espera da escolha dos itens pelo Jev fica:
    // sai com o número que ganhou quando a decisão chega.
    let project_root = crate::commands::spec_events::project(start).root;
    let undone = match asker {
        Some(asker) => &undone - &super::item_choice::held_for_the_jev(&project_root, spec, log, &undone, asker),
        None => undone,
    };
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
    // As ondas do plano que ainda não terminaram: em andamento, por sair,
    // mantidas ou com conserto pendente. Elas vão ao Jev e seguram a limpeza
    // que pegaria carona, nunca uma tarefa pelo arquivo.
    let open_waves = open_plan_waves(log, &undone);
    // As vagas que as ondas já montadas deixam livres. O ciclo entre ondas é
    // recusado pela escolha das ondas prontas, logo depois: aqui só não sobra
    // vaga para onda nova.
    let vacancies = super::vacancy::vacancies_with(&project_root, spec, log, limit);
    let (free, alone) = outgoing(log, &vacancies, &waves_stuck(log), &undone).map_or((0, false), |out| (out.free, out.alone));

    let code_of = |id: &u64| -> u64 { codes.get(id).and_then(|code| code.rsplit('-').next()).and_then(|number| number.parse().ok()).unwrap_or(*id) };
    // Cada resumo que ainda vale vira a base de uma onda, antes de qualquer outra;
    // as tarefas dessas ondas ficam fora do resto da montagem, saiam elas
    // agora ou esperem a vez: a que saísse noutra onda perderia o resumo.
    // Antes delas, cada onda que o classificador formou sai inteira, com as
    // tarefas na posição que ele deu e, como base, o resumo que deixou alguma
    // delas; o resumo só forma onda própria com as tarefas que nenhuma onda do
    // classificador levou, e o resto vai à montagem de sempre.
    let by_group = group_waves(log, &order, &population);
    let grouped: BTreeSet<u64> = by_group.iter().flat_map(|(batch, _)| batch.tasks.iter().copied()).collect();
    let order: Vec<u64> = order.into_iter().filter(|id| !grouped.contains(id)).collect();
    let by_summary = summary_waves(log, &order, &population);
    let claimed: BTreeSet<u64> = by_summary.iter().flat_map(|wave| wave.batch.tasks.iter().copied()).collect();
    let order: Vec<u64> = order.into_iter().filter(|id| !claimed.contains(id)).collect();
    let tidy: Vec<_> = crate::shared::dag::ready_tasks(&population).into_iter()
        .filter(|id| left_on_entry.contains(id) && cleanup(id) && !bare(id)).collect();
    let candidates: Vec<_> = order.iter().chain(&waiting).chain(&claimed).chain(&tidy).copied().collect();
    let mut affinity_board = board_of(log, &by_id, &population, &candidates, &open_waves);
    if free > 0 && (!order.is_empty() || !claimed.is_empty()) {super::evidence::augment(start, &mut affinity_board);}
    // Only uncached typed decisions are delegated when a slot is available.
    // Native source relationships are reused in the paid board.
    let eligible = order.iter().filter_map(|id| population.iter().find(|task| task.id == *id))
        .any(|task| !touches_whole_tree(&task.files) || alone);
    let judging = match judge {
        Some(judge) if free > 0 && eligible => {
            // Only dependent candidates that can become eligible in this
            // batch need a profile. Their intrinsic cache key is independent
            // of the running board, just like already-ready tasks.
            let mut candidates: BTreeSet<u64> = order.iter().copied().collect();
            let done: BTreeSet<u64> = population.iter().filter(|t| t.done).map(|t| t.id).collect();
            loop {
                let before = candidates.len();
                for task in population.iter().filter(|t| waiting.contains(&t.id)) {
                    if task.depends_on.iter().all(|d| done.contains(d) || candidates.contains(d)) {
                        candidates.insert(task.id);
                    }
                }
                if before == candidates.len() {
                    break;
                }
            }
            let mut board = board_of(log, &by_id, &population, &candidates.into_iter().collect::<Vec<_>>(), &open_waves);
            for task in &mut board.backlog {
                if let Some(known) = affinity_board.backlog.iter().find(|known| known.id == task.id) {task.reads.clone_from(&known.reads);}
            }
            board.running.clone_from(&affinity_board.running);
            // A chamada paga só sai com candidatas bastante para a escolha
            // dela importar. Com duas ou menos, a regra de arquivos já diz se
            // elas saem juntas, e a chamada só gasta.
            crate::shared::jev::worth_asking(&board).then(|| judged_board(&board, judge, asker))
        }
        _ => None,
    };
    let undecided: BTreeSet<u64> = judging.as_ref().map(|judging| judging.undecided.clone()).unwrap_or_default();
    let batches = match &judging {
        _ if formed_only => Vec::new(),
        Some(Judging { judged: Some(judged), .. }) => {
            // A tarefa sem decisão do Jev espera a rodada seguinte, só ela.
            let decided = |ids: &[u64]| -> Vec<u64> { ids.iter().copied().filter(|id| !undecided.contains(id)).collect() };
            pack_by_kind(&population, &decided(&order), &decided(&waiting), &judged.tasks, &code_of)
        }
        _ => {
            let mut batches = if order.is_empty() { Vec::new() } else {
                pack_by_local_evidence(&population, &order, &waiting, &super::evidence::links(&affinity_board))
            };
            // O lote com a tarefa marcada como prioridade primeiro; depois o
            // curinga da árvore inteira; os outros pela tarefa de código mais
            // baixo, e a ordem de prontidão do motor desempata.
            batches.sort_by_key(|batch| (!batch.holds_any(&marked), !touches_whole_tree(&batch.files), batch.tasks.iter().map(&code_of).min()));
            batches
        }
    };
    let batches: Vec<(Batch<u64>, Option<u64>)> =
        by_group
            .into_iter()
            .chain(by_summary.into_iter().filter(|_| !formed_only).map(|wave| (wave.batch, Some(wave.summary))))
            .chain(batches.into_iter().map(|batch| (batch, None)))
            .collect();
    // Cada lote ocupa uma vaga, na ordem de prioridade, divida ele arquivo
    // com outro lote, com uma onda em andamento ou com ninguém: o que não
    // cabe nas vagas fica no backlog e entra na montagem da rodada em que uma
    // vaga abrir.
    let mut chosen: Vec<(Batch<u64>, Option<u64>)> = Vec::new();
    for (batch, summary) in batches {
        if chosen.len() >= free {
            break;
        }
        if touches_whole_tree(&batch.files) {
            // O curinga da árvore inteira cruza com todos: só sai sozinho, sem
            // nada em andamento. Ele não segura nada, senão parava todo o
            // resto atrás dele.
            if alone && chosen.is_empty() {
                chosen.push((batch, summary));
                break;
            }
            continue;
        }
        chosen.push((batch, summary));
    }
    // A chamada da montagem fica gravada, com quantas tarefas foram ao Jev e
    // quantas saíram em onda; na falha, com o motivo, e a montagem seguiu
    // pelo arquivo.
    if let Some(Judging { asked, call: Some((called, answer)), .. }) = &judging {
        let returned = chosen.iter().filter(|(_, summary)| summary.is_none()).map(|(batch, _)| batch.tasks.len()).sum();
        record_jev_call(start, spec, ASSEMBLY_CALL, *called, Some((*asked, returned)), answer.as_ref());
    }

    // Attach a ready, small cleanup to the last functional batch when local
    // source evidence links it. Never add unrelated work or reopen active waves.
    let complete = !formed_only && open_waves.is_empty() && left_on_entry.iter().filter(|id| !cleanup(id))
        .all(|id| chosen.iter().any(|(batch, _)| batch.tasks.contains(id)));
    if complete {
        let links = super::evidence::links(&affinity_board);
        for id in tidy {
            let Some(task) = population.iter().find(|task| task.id == id && task.files.len() == 1) else { continue; };
            if chosen.iter().any(|(batch, _)| batch.tasks.contains(&id)) { continue; }
            let target = chosen.iter().position(|(batch, _)| !touches_whole_tree(&batch.files)
                && (sets_cross(&task.files, &batch.files) || batch.tasks.iter().any(|other| links.contains(&(id,*other)) || links.contains(&(*other,id)))));
            let Some(at) = target else { continue; };
            if chosen.iter().enumerate().any(|(other,(batch,_))| other != at && sets_cross(&task.files, &batch.files)) { continue; }
            chosen[at].0.tasks.push(id);
            chosen[at].0.files.extend(task.files.iter().cloned());
        }
    }
    let judged = match &judging {
        Some(Judging { judged: Some(judged), .. }) => Some(&judged.tasks),
        _ => None,
    };
    let assembled = Assembled { codes: &codes, claimed: &claimed, order: &order, chosen: &chosen };
    let mut held = why_held(&assembled, judged, arrived);
    for id in &undecided {
        held.insert(codes.get(id).cloned().unwrap_or_else(|| id.to_string()), Hold::JevPending);
    }

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
    // As tarefas ainda abertas: as do backlog, as dos lotes montados agora e
    // as das ondas planejadas que não terminaram.
    let open = tasks_not_delivered(log, &BTreeSet::new());
    for (batch, summary) in &chosen {
        next_n += 1;
        let tasks: Vec<&SpecEvent> = batch.tasks.iter().filter_map(|id| by_id.get(id).copied()).collect();
        // A tarefa do plano cobre critério, com prova para juntar aqui; a
        // que nasce de uma sobra cobre o pedido dela, pronto pela prova que
        // ela nomeia ou pela compilação; a que nasce do item combinado sem
        // onda dona (o backlog da revisão final) cobre o item, que não tem
        // prova — o texto de quem marcou o item sem atender é o que diz
        // quando a onda entrega. A prova do item que outra tarefa aberta,
        // fora deste lote, ainda cobre fica de fora: ela vai à onda da
        // última tarefa aberta dele, e o fechamento roda todas.
        let waiting: BTreeSet<u64> =
            open.iter().filter(|id| !batch.tasks.contains(id)).filter_map(|id| log.get(*id)).flat_map(|task| task.ints("covers")).collect();
        let fields = mustard_core::domain::wave_prompt::backlog_fields(log, &tasks, &commands, &waiting);
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
        return Ok(((formed, held), dry.then(|| locked.clone())));
    }
    let mut check = RecordCheck::open(start, spec, PhaseWriter::Binary)?;
    for (event_type, draft) in &writes {
        check.record(event_type, draft.clone())?;
    }
    if dry {
        return Ok(((formed, held), Some(check.log().clone())));
    }
    for (event_type, draft) in writes {
        record(start, spec, event_type, draft, PhaseWriter::Binary)?;
    }
    Ok(((formed, held), None))
}

/// O que a montagem decidiu, para dizer por que cada tarefa pronta ficou: o
/// código de cada tarefa, as presas a um resumo (`claimed`) e as prontas
/// (`order`), na ordem de prioridade, e as ondas que saem (`chosen`).
struct Assembled<'a> {
    codes: &'a BTreeMap<u64, String>,
    claimed: &'a BTreeSet<u64>,
    order: &'a [u64],
    chosen: &'a [(crate::shared::dag::Batch<u64>, Option<u64>)],
}

/// O motivo de cada tarefa pronta que não saiu, pelo código dela: o choque
/// que o Jev julgou (`judged`) com uma onda em andamento, ou a chegada ao
/// backlog nesta rodada (`arrived`). A que ficou só pela vaga não ganha motivo
/// aqui: o quadro da fila já o diz. Nenhuma fica pelo arquivo de outra tarefa
/// ou de uma onda.
fn why_held(at: &Assembled<'_>, judged: Option<&BTreeMap<u64, crate::shared::dag::Judgement>>, arrived: Vec<u64>) -> BTreeMap<String, Hold> {
    let out: BTreeSet<u64> = at.chosen.iter().flat_map(|(batch, _)| batch.tasks.iter().copied()).collect();
    let mut held: BTreeMap<u64, Hold> = arrived.into_iter().map(|id| (id, Hold::Arrived)).collect();
    for id in at.claimed.iter().chain(at.order).filter(|id| !out.contains(id)) {
        let chance = judged.and_then(|judged| judged.get(id)).map(|judgement| judgement.clash).filter(|clash| *clash >= CLASH_FROM);
        if let Some(clash) = chance {
            held.insert(*id, Hold::Judged((clash * 100.0).round() as u8));
        }
    }
    let code = |id: &u64| at.codes.get(id).cloned().unwrap_or_else(|| id.to_string());
    held.into_iter().map(|(id, hold)| (code(&id), hold)).collect()
}

/// A obra da spec lida em `log`, como o quadro a mostra: o tipo do prefixo da
/// branch e o objetivo que o usuário gravou. Um deles pode faltar, e o quadro
/// manda o que tem.
fn work_of(log: &SpecLog) -> crate::shared::judgement::BoardWork {
    use mustard_core::domain::survey;
    let kind = mustard_core::domain::spec_state::State::from_log(log).branch.as_deref().and_then(crate::shared::work_kind::WorkKind::of_branch).map(|kind| kind.token().to_string());
    let goal = survey::goal(log).and_then(|goal| goal.str_field("text").map(str::to_string)).filter(|text| !text.trim().is_empty());
    crate::shared::judgement::BoardWork { kind, goal }
}

/// O quadro que o Jev recebe: a obra de que a montagem é ([`work_of`]), as
/// ondas do plano ainda abertas (`open_waves`), cada uma com as tarefas e os
/// arquivos delas, e o backlog pronto (`ready`), na ordem de prontidão, com o
/// que cada tarefa diz de si e os títulos das tarefas de que depende.
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
    Board { work: work_of(log), running, backlog }
}

/// As ondas do plano que ainda não terminaram — em andamento, por sair,
/// mantidas ou com conserto pendente —, fora as ondas de lote `undone`, que
/// a montagem desfaz. O Jev as vê como as ondas em andamento, e a limpeza só
/// pega carona sem nenhuma delas; o arquivo delas não segura tarefa nenhuma.
fn open_plan_waves(log: &SpecLog, undone: &BTreeSet<u64>) -> Vec<u64> {
    let done = waves_done(log, &waves_in_progress(log));
    log.planned_waves().into_iter().filter(|n| !done.contains(n) && !undone.contains(n)).collect()
}

/// As ondas de lote do plano que nenhum envio despachou e nenhuma entrega
/// fechou: montadas pelo backlog e não enviadas. É de lote a onda que
/// [`dispatch_backlog`] grava, com a ordem das tarefas no campo `order` e
/// autor binário; a onda combinada à mão no plano, sem ordem, segue como está
/// até a rodada despachá-la. É a onda de lote que a rodada desfaz.
fn unsent_batch_waves(log: &SpecLog, observed: &[SpecEvent]) -> BTreeSet<u64> {
    use mustard_core::io::spec_index::later;
    // A rodada recusada é a chamada mais nova que o programa observou; ela
    // mora fora da spec, então o antes e o depois se dizem pela hora.
    let failure=observed.iter().rfind(|e|e.event_type=="call" && e.str_field("command")==Some("round") && e.str_field("result")==Some("refused"))
        .map(SpecEvent::at);
    let revised=log.visible().into_iter().filter(|e|e.str_field("author")!=Some("binary") && ["task","request","criterion","wave"].contains(&e.event_type.as_str()))
        .map(SpecEvent::at).reduce(|newest,at| if later(at,newest) {at} else {newest});
    let sent: BTreeSet<u64> = log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "send").filter_map(SpecEvent::wave).collect();
    let delivered = log.delivered_waves();
    let formed: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "wave" && e.str_field("author") == Some("binary") && !e.ints("order").is_empty())
        .filter_map(SpecEvent::wave)
        .collect();
    log.planned_waves().into_iter().filter(|n| {
        let recoverable=failure.is_some_and(|failed| revised.is_none_or(|revised| later(failed,revised))
            && log.visible().into_iter().any(|e|e.event_type=="wave" && e.wave()==Some(*n) && later(failed,e.at())));
        formed.contains(n) && !sent.contains(n) && !delivered.contains(n) && !recoverable
    }).collect()
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::spec_events::SpecLog;
    use mustard_core::io::spec_events as store;
    use serde_json::json;
    use tempfile::tempdir;

    use super::super::tests::{approved, returned, round, waves_in};

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

    /// Num projeto TypeScript, a onda 1 está em andamento mudando o contrato
    /// `src/contract.ts`. Cinco tarefas pequenas, sem dependência, ficam
    /// prontas: duas no mesmo contrato, uma no consumidor que, pelo mapa do
    /// scan, lê esse contrato, e duas em arquivos à parte. As cinco, do mesmo
    /// tipo, saem nesta rodada ao lado da onda 1, numa onda só, sem teto, e
    /// nenhuma fica com motivo; só a sexta, que depende da tarefa da onda 1,
    /// espera.
    #[test]
    fn ready_tasks_leave_beside_a_running_wave_on_their_file_and_only_the_dependent_waits() {
        use super::super::queue::{backlog_project, backlog_task_on, max_parallel, seed_running};
        use super::super::tests::{id_of, write};
        use crate::shared::dag::{Judgement, TaskKind};
        use crate::shared::judgement::{Board, Judged};

        let scan = mustard_core::Scan::locate();
        assert!(
            scan.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        for (file, text) in [
            ("src/contract.ts", "export interface Order { id: number }\n"),
            ("src/consumer.ts", "import { Order } from './contract';\nexport function total(order: Order): number { return order.id; }\n"),
            ("src/label.ts", "export const label = 'total';\n"),
            ("src/page.ts", "export const title = 'orders';\n"),
        ] {
            std::fs::write(root.join(file), text).unwrap();
        }
        let first = backlog_task_on(root, said, crit, "Change the contract.", &["src/contract.ts"]);
        let log = spec_x(root);
        assert_eq!(super::dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1]));
        seed_running(root, 1);
        scan.scan(root, &mustard_core::io::project_map::model_path(root)).expect("the scan writes the map");

        let ready = [
            backlog_task_on(root, said, crit, "Rename the order id.", &["src/contract.ts"]),
            backlog_task_on(root, said, crit, "Add the order date.", &["src/contract.ts"]),
            backlog_task_on(root, said, crit, "Change the total.", &["src/consumer.ts"]),
            backlog_task_on(root, said, crit, "Change the label.", &["src/label.ts"]),
            backlog_task_on(root, said, crit, "Change the title.", &["src/page.ts"]),
        ];
        let body = json!({"text": "Use the new contract.", "files": [{"path": "src/label.ts"}], "depends_on": [first], "covers": [crit], "origin": said});
        let dependent = id_of(&write(root, "x", "task", body));
        // Todas pequenas e do mesmo tipo, sem choque com a onda em andamento.
        let judge = |board: &Board| {
            let small = Judgement { kind: TaskKind::Feature, confidence: 0.9, clash: 0.0 };
            Ok(Judged { tasks: board.backlog.iter().map(|task| (task.id, small)).collect(), usage: Default::default() })
        };

        let log = spec_x(root);
        let (formed, held) = super::dispatch_backlog(root, "x", &log, &log, max_parallel(root), (Some(&judge), None)).unwrap();
        assert_eq!(formed, vec![2], "as cinco do mesmo tipo saem na onda 2, sem teto");
        let now = spec_x(root);
        let wave_of = |task: u64| now.current(task).and_then(mustard_core::domain::spec_events::SpecEvent::wave);
        assert_eq!(ready.map(wave_of), [Some(2); 5], "nenhuma espera pelo arquivo nem pelo contrato da onda 1");
        assert_eq!(wave_of(dependent), None, "a que depende da tarefa da onda 1 espera");
        assert!(held.is_empty(), "nenhuma pronta ficou com motivo: {held:?}");
    }
}
