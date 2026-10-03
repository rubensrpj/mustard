//! A escolha dos itens do pedido de cada onda que sai. Os itens que o pedido
//! não leva nem tira por conta própria passam pelo Jev, uma chamada por onda:
//! para cada item, ele diz se o item governa algo que as tarefas da onda mudam
//! ou testam. O código decide pelas chances (`Choice::by_chances`): o item do
//! projeto todo só sai quando o Jev tem quase certeza de que ele não serve, e
//! o item sem ligação com a onda só entra quando tem boa certeza de que serve.
//! A onda não espera ninguém: sai na mesma rodada. Sem Jev — sem chave ou
//! desligado — ou com a chamada falhando, nenhuma escolha se grava, e o pedido
//! leva o padrão: o projeto todo vai, e o sem ligação fica fora.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use mustard_core::domain::map_filter::FilterError;
use mustard_core::domain::spec_events::{BlockQuery, SpecEvent, SpecLog};
use mustard_core::domain::wave_prompt::{candidates, item_title, Candidates, Choice};
use serde_json::{json, Map};

use crate::commands::spec_events::conversation::record_measured_call;
use crate::shared::jev::{BoardItem, BoardTask, ItemsBoard, ItemsJudged, JevFilter};

/// Quem julga os itens de uma onda: uma chamada com o quadro dela
/// ([`ItemsBoard`]), que volta com a chance de sim de cada item. As chamadas
/// de várias ondas correm ao mesmo tempo.
pub(crate) type JudgeItems<'a> = dyn Fn(&ItemsBoard) -> Result<ItemsJudged, FilterError> + Sync + 'a;

/// A escolha dos itens de cada onda pronta (`ready`) que tem candidato, pelo
/// número da onda, julgada pelo Jev (`jev`). A onda sem candidato, e toda onda
/// sem Jev, ficam de fora: o pedido dela leva o padrão. Cada chamada grava um
/// evento `call` com o tempo, os tokens, o custo e o modelo; a que falha o
/// grava com o motivo, e a onda dela segue pelo padrão.
pub(super) fn choose_items(
    start: &Path,
    spec: &str,
    log: &SpecLog,
    ready: &[u64],
    jev: Option<&JevFilter>,
) -> BTreeMap<u64, Choice> {
    #[cfg(test)]
    if let Some(fake) = fake::installed() {
        return choose_with(start, spec, log, ready, Some(&*fake));
    }
    let judge = jev.map(|jev| move |board: &ItemsBoard| jev.judge_items(board));
    choose_with(start, spec, log, ready, judge.as_ref().map(|judge| judge as &JudgeItems<'_>))
}

/// [`choose_items`] com quem julga (`judge`) já escolhido.
fn choose_with(
    start: &Path,
    spec: &str,
    log: &SpecLog,
    ready: &[u64],
    judge: Option<&JudgeItems<'_>>,
) -> BTreeMap<u64, Choice> {
    let Some(judge) = judge else { return BTreeMap::new() };
    let asked: Vec<(u64, Candidates<'_>, ItemsBoard)> = ready
        .iter()
        .filter_map(|wave| {
            let found = candidates(log, *wave);
            (!found.is_empty()).then(|| {
                let board = board_of(log, *wave, &found);
                (*wave, found, board)
            })
        })
        .collect();
    let answers: Vec<(Instant, Result<ItemsJudged, FilterError>)> = std::thread::scope(|scope| {
        let running: Vec<_> = asked
            .iter()
            .map(|(_, _, board)| {
                scope.spawn(move || {
                    let called = Instant::now();
                    (called, judge(board))
                })
            })
            .collect();
        running
            .into_iter()
            .map(|handle| {
                handle.join().unwrap_or_else(|_| {
                    (Instant::now(), Err(FilterError::Network("a request thread failed".to_string())))
                })
            })
            .collect()
    });
    let mut choices = BTreeMap::new();
    for ((wave, found, _), (called, answer)) in asked.iter().zip(&answers) {
        let choice = answer.as_ref().ok().map(|judged| Choice::by_chances(found, &judged.chances));
        let changed = choice.as_ref().map_or(0, |choice| choice.removed.len() + choice.added.len());
        record_call(start, spec, (found.ids().len(), changed), *called, answer);
        if let Some(choice) = choice {
            choices.insert(*wave, choice);
        }
    }
    choices
}

/// O quadro da onda `wave`: as tarefas dela e os candidatos (`found`), cada um
/// com o título e o texto, sem dono, arquivo nem onda.
fn board_of(log: &SpecLog, wave: u64, found: &Candidates<'_>) -> ItemsBoard {
    let tasks = log
        .block(BlockQuery::Wave(wave))
        .into_iter()
        .filter(|e| e.event_type == "task")
        .map(|task| BoardTask::of(task, Vec::new()))
        .collect();
    let shown = |item: &&SpecEvent| BoardItem {
        id: item.id,
        title: item_title(item).unwrap_or_default(),
        text: item.str_field("text").unwrap_or_default().to_string(),
    };
    let mut items: Vec<BoardItem> = found.project.iter().chain(&found.unlinked).map(shown).collect();
    items.sort_by_key(|item| item.id);
    ItemsBoard { tasks, items }
}

/// Grava o evento `call` da escolha dos itens, como a busca grava o dela:
/// quantos itens foram ao Jev e quantos mudaram no pedido (`asked`,
/// `changed`) e, na resposta, o tempo, os tokens, o custo e o
/// modelo; na falha, o motivo dela no nome do filtro (`jev:<motivo>`).
fn record_call(
    start: &Path,
    spec: &str,
    (asked, changed): (usize, usize),
    called: Instant,
    answer: &Result<ItemsJudged, FilterError>,
) {
    let mut measured = Map::new();
    measured.insert("candidates".to_string(), json!(asked));
    measured.insert("returned".to_string(), json!(changed));
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
    let _ = record_measured_call(start, "wave items", Some(spec), None, called, &report, measured);
}

/// O Jev de mentira dos testes da biblioteca: a rodada dos testes nunca chama o
/// serviço, e quem quer uma escolha instala aqui quem responde.
#[cfg(test)]
pub(crate) mod fake {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use mustard_core::domain::map_filter::{FilterError, FilterUsage};

    use crate::shared::jev::{ItemsBoard, ItemsJudged};

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
            let usage = FilterUsage {
                input_tokens: 1_000,
                cost_micro_usd: 42,
                requests: 1,
                model: "jev-1.13.0".to_string(),
                ..FilterUsage::default()
            };
            Ok(ItemsJudged { chances: chances(board), usage })
        });
        INSTALLED.with(|installed| *installed.borrow_mut() = Some(answer));
        Installed
    }

    /// O Jev de mentira que recusa toda chamada.
    pub(crate) fn refusing() -> Installed {
        let answer: Arc<Answer> = Arc::new(|_: &ItemsBoard| Err(FilterError::Refused { status: 529 }));
        INSTALLED.with(|installed| *installed.borrow_mut() = Some(answer));
        Installed
    }
}
