//! A escolha dos itens do pedido de cada onda que sai. Os itens que o pedido
//! não leva nem tira por conta própria passam pelo Jev, uma chamada por onda:
//! para cada item, ele diz se o item governa algo que as tarefas da onda mudam
//! ou testam. O código decide pelas chances (`Choice::by_chances`): o item que
//! o pedido leva por padrão — o do projeto todo e o dos arquivos que as
//! tarefas da onda mexem — sai quando o Jev dá a ele menos de 20% de chance de
//! servir, e o item sem ligação com a onda só entra quando o Jev tem alta
//! certeza — 85% de chance ou mais — de que serve. O item que uma tarefa da
//! onda faz, o de que a onda é dona e o de toda onda vão sempre, sem passar
//! pelo Jev.
//! A pergunta sai antes das travas da rodada, e a resposta fica gravada
//! ([`super::dispatch_jev`]); com as travas presas, a rodada só a lê, e a onda
//! sem decisão espera a rodada seguinte, só ela. O Jev que falha manda a
//! pergunta à reserva, e a onda espera o recibo. Sem Jev — sem chave ou
//! desligado —, com a reserva incerta, ou com o quadro grande demais, nenhuma
//! escolha se grava, e o pedido leva o padrão: o projeto todo e os dos
//! arquivos da onda vão, e o sem ligação fica fora.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use mustard_core::domain::map_filter::FilterError;
use mustard_core::domain::spec_events::{BlockQuery, SpecEvent, SpecLog};
use mustard_core::domain::wave_prompt::{Candidates, Choice, candidates, item_title};
use serde_json::{Map, Value, json};

use super::dispatch_jev::{Asker, Outcome, unsure};
use crate::shared::jev::{ITEMS_CALL, items_payloads, read_item_chances, record_jev_call};
use crate::shared::judgement::{BoardItem, BoardTask, ItemsBoard, ItemsJudged, WaveJudge};

/// Quem julga os itens de uma onda: uma chamada com o quadro dela
/// ([`ItemsBoard`]), que volta com a chance de sim de cada item. As chamadas
/// de várias ondas correm ao mesmo tempo.
pub(crate) type JudgeItems<'a> = dyn Fn(&ItemsBoard) -> Result<ItemsJudged, FilterError> + Sync + 'a;

/// A escolha dos itens de cada onda pronta (`ready`) que tem candidato, pelo
/// número da onda, julgada pelo Jev (`jev`) e decidida por `asker`: antes das
/// travas, a chamada sai e a resposta fica gravada; com elas presas, só se lê
/// a decisão gravada. A onda sem candidato, e toda onda sem Jev, ficam de
/// fora: o pedido dela leva o padrão, e também o da onda cuja decisão veio
/// incerta da reserva. Volta também as ondas que esperam a decisão. Cada
/// chamada grava um evento `call` com o tempo, os tokens, o custo e o modelo;
/// a que falha o grava com o motivo.
pub(super) fn choose_items(start: &Path, spec: &str, log: &SpecLog, ready: &[u64], jev: Option<&dyn WaveJudge>, asker: &Asker<'_>) -> (BTreeMap<u64, Choice>, BTreeSet<u64>) {
    #[cfg(test)]
    if let Some(fake) = fake::installed() {
        return choose_with(start, spec, log, ready, Some(&*fake), asker);
    }
    let judge = jev.map(|jev| move |board: &ItemsBoard| jev.judge_items(board));
    choose_with(start, spec, log, ready, judge.as_ref().map(|judge| judge as &JudgeItems<'_>), asker)
}

/// Se a escolha dos itens pergunta ao Jev nesta rodada: com o Jev `jev`, ou,
/// nos testes, com o de mentira instalado, como em [`choose_items`].
pub(super) fn asks_jev(jev: Option<&dyn WaveJudge>) -> bool {
    #[cfg(test)]
    if fake::installed().is_some() {
        return true;
    }
    jev.is_some()
}

/// As ondas de `waves`, montadas pelo backlog e não enviadas, na spec lida em
/// `log`, que a rodada que as montou segurou à espera da escolha dos itens
/// pelo Jev do projeto em `root`: algum pedido da escolha dela não tinha
/// decisão gravada, por `asker`, quando a onda foi gravada. A montagem não as
/// desfaz: cada uma sai com o número que ganhou, na rodada em que a decisão
/// chega. A onda que já tinha a decisão e não saiu por outro motivo fica de
/// fora, e todas ficam com o Jev desligado.
pub(super) fn held_for_the_jev(root: &Path, spec: &str, log: &SpecLog, waves: &BTreeSet<u64>, asker: &Asker<'_>) -> BTreeSet<u64> {
    if waves.is_empty() || !asks_jev(crate::shared::jev::for_context(root, spec).filter.as_deref()) {
        return BTreeSet::new();
    }
    let held = |wave: u64| -> Option<bool> {
        let formed = log.block(BlockQuery::Wave(wave)).into_iter().filter(|e| e.event_type == "wave").map(|e| e.id).max()?;
        let (_, board) = board_for(log, wave)?;
        let parts = items_payloads(&board).ok()?;
        let payloads: Vec<String> = parts.into_iter().map(|(payload, _)| payload).collect();
        Some(asker.undecided_at(ITEMS_CALL, &payloads, formed))
    };
    waves.iter().copied().filter(|wave| held(*wave) == Some(true)).collect()
}

/// A pergunta de uma onda: o número, os candidatos, o quadro e os pedidos ao
/// Jev, cada um com os itens que pergunta.
type Asked<'l> = (u64, Candidates<'l>, ItemsBoard, Vec<(String, Vec<u64>)>);

/// As respostas, na forma das do Jev, aos pedidos `parts` de uma onda, com a
/// chance que `judged` deu a cada item.
fn docs_of(parts: &[(String, Vec<u64>)], judged: &ItemsJudged) -> Option<Vec<Value>> {
    parts
        .iter()
        .map(|(_, ids)| {
            let answers: Option<Map<String, Value>> =
                ids.iter().map(|id| Some((format!("i{id}"), json!({ "type": "noul", "noul": judged.chances.get(id)? })))).collect();
            Some(json!({ "answers": answers? }))
        })
        .collect()
}

/// [`choose_items`] com quem julga (`judge`) já escolhido.
fn choose_with(start: &Path, spec: &str, log: &SpecLog, ready: &[u64], judge: Option<&JudgeItems<'_>>, asker: &Asker<'_>) -> (BTreeMap<u64, Choice>, BTreeSet<u64>) {
    let Some(judge) = judge else {
        return (BTreeMap::new(), BTreeSet::new());
    };
    let boards: Vec<(u64, Candidates<'_>, ItemsBoard)> =
        ready.iter().filter_map(|wave| board_for(log, *wave).map(|(found, board)| (*wave, found, board))).collect();
    // O quadro que passa do que o serviço aceita leva o padrão, sem chamada;
    // antes das travas, a falha fica gravada com o motivo.
    let mut asked: Vec<Asked<'_>> = Vec::new();
    for (wave, found, board) in boards {
        match items_payloads(&board) {
            Ok(parts) => asked.push((wave, found, board, parts)),
            Err(error) if asker.asks() => record_jev_call(start, spec, ITEMS_CALL, Instant::now(), Some((found.ids().len(), 0)), Err(&error)),
            Err(_) => {}
        }
    }
    let payloads_of = |parts: &[(String, Vec<u64>)]| -> Vec<String> { parts.iter().map(|(payload, _)| payload.clone()).collect() };
    if asker.asks() {
        // Antes das travas: o recibo da reserva vale primeiro; as ondas ainda
        // sem decisão vão ao Jev ao mesmo tempo, e a resposta de cada uma fica
        // gravada, ou vai à reserva.
        let calling: Vec<&Asked<'_>> = asked
            .iter()
            .filter(|(_, _, _, parts)| {
                let payloads = payloads_of(parts);
                !asker.missing(ITEMS_CALL, &payloads).is_empty() && !asker.take_receipt(ITEMS_CALL, &payloads)
            })
            .collect();
        let answers: Vec<(Instant, Result<ItemsJudged, FilterError>)> = std::thread::scope(|scope| {
            let running: Vec<_> = calling
                .iter()
                .map(|(_, _, board, _)| {
                    scope.spawn(move || {
                        let called = Instant::now();
                        (called, judge(board))
                    })
                })
                .collect();
            running
                .into_iter()
                .map(|handle| handle.join().unwrap_or_else(|_| (Instant::now(), Err(FilterError::Network("a request thread failed".to_string())))))
                .collect()
        });
        for ((_, found, _, parts), (called, answer)) in calling.iter().zip(&answers) {
            let choice = answer.as_ref().ok().map(|judged| Choice::by_chances(found, &judged.chances));
            let changed = choice.as_ref().map_or(0, |choice| choice.removed.len() + choice.added.len());
            record_jev_call(start, spec, ITEMS_CALL, *called, Some((found.ids().len(), changed)), answer.as_ref().map(|judged| &judged.usage));
            let docs = answer.as_ref().ok().and_then(|judged| docs_of(parts, judged));
            asker.settle(ITEMS_CALL, &payloads_of(parts), docs, true, *called);
        }
    }
    let mut choices = BTreeMap::new();
    let mut waiting = BTreeSet::new();
    for (wave, found, _, parts) in &asked {
        match asker.decided(ITEMS_CALL, &payloads_of(parts)).unwrap_or(Outcome::Waiting) {
            Outcome::Answered(docs) if !unsure(&docs) => {
                let mut chances = BTreeMap::new();
                if parts.iter().zip(&docs).all(|((_, ids), doc)| read_item_chances(doc, ids, &mut chances).is_ok()) {
                    choices.insert(*wave, Choice::by_chances(found, &chances));
                }
            }
            Outcome::Answered(_) | Outcome::Rule => {}
            Outcome::Waiting => {
                waiting.insert(*wave);
            }
        }
    }
    (choices, waiting)
}

/// Os candidatos da onda `wave` e o quadro deles ([`board_of`]); `None` na
/// onda sem candidato, que leva o pedido padrão. É o quadro que a escolha dos
/// itens ([`choose_with`]) e a espera por ela ([`held_for_the_jev`]) perguntam,
/// montado num lugar só para as duas acharem a mesma decisão.
fn board_for(log: &SpecLog, wave: u64) -> Option<(Candidates<'_>, ItemsBoard)> {
    let found = candidates(log, wave);
    if found.is_empty() {
        return None;
    }
    let board = board_of(log, wave, &found);
    Some((found, board))
}

/// O quadro da onda `wave`: as tarefas dela e os candidatos (`found`), cada um
/// com o título e o texto, sem dono, arquivo nem onda: o Jev julga o item
/// ligado aos arquivos da onda como o do projeto todo.
fn board_of(log: &SpecLog, wave: u64, found: &Candidates<'_>) -> ItemsBoard {
    let tasks = log.block(BlockQuery::Wave(wave)).into_iter().filter(|e| e.event_type == "task").map(|task| BoardTask::of(task, Vec::new())).collect();
    let shown = |item: &&SpecEvent| BoardItem {
        id: item.id,
        title: item_title(item).unwrap_or_default(),
        text: item.str_field("text").unwrap_or_default().to_string(),
    };
    let mut items: Vec<BoardItem> = found.carried.iter().chain(&found.unlinked).map(shown).collect();
    items.sort_by_key(|item| item.id);
    ItemsBoard { tasks, items }
}

/// O Jev de mentira dos testes da biblioteca: a rodada dos testes nunca chama o
/// serviço, e quem quer uma escolha instala aqui quem responde.
#[cfg(test)]
pub(crate) mod fake {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use mustard_core::domain::map_filter::{FilterError, FilterUsage};

    use crate::shared::judgement::{ItemsBoard, ItemsJudged};

    type Answer = dyn Fn(&ItemsBoard) -> Result<ItemsJudged, FilterError> + Send + Sync;

    thread_local! {
        static INSTALLED: RefCell<Option<Arc<Answer>>> = const { RefCell::new(None) };
    }

    /// Quem responde na thread de agora, quando há.
    pub(super) fn installed() -> Option<Arc<Answer>> {
        INSTALLED.with(|installed| installed.borrow().clone())
    }

    /// O que desinstala o Jev de mentira ao sair de escopo.
    pub(crate) struct Installed;

    impl Drop for Installed {
        fn drop(&mut self) {
            INSTALLED.with(|installed| installed.borrow_mut().take());
        }
    }

    /// O Jev de mentira que responde a cada quadro com as chances que
    /// `chances` dá, pelo número do item, e conta 1.000 tokens de entrada.
    pub(crate) fn answering(chances: impl Fn(&ItemsBoard) -> BTreeMap<u64, f64> + Send + Sync + 'static) -> Installed {
        let answer: Arc<Answer> = Arc::new(move |board: &ItemsBoard| {
            let usage = FilterUsage { input_tokens: 1_000, cost_micro_usd: 42, requests: 1, model: "jev-1.13.0".to_string(), ..FilterUsage::default() };
            Ok(ItemsJudged { chances: chances(board), usage })
        });
        INSTALLED.with(|installed| *installed.borrow_mut() = Some(answer));
        Installed
    }

    /// O Jev de mentira que recusa toda chamada.
    pub(crate) fn refusing() -> Installed {
        refusing_with(529)
    }

    /// O Jev de mentira que recusa toda chamada com o código HTTP `status`.
    pub(crate) fn refusing_with(status: u16) -> Installed {
        let answer: Arc<Answer> = Arc::new(move |_: &ItemsBoard| Err(FilterError::Refused { status }));
        INSTALLED.with(|installed| *installed.borrow_mut() = Some(answer));
        Installed
    }
}
