//! A resposta de uma volta pelo combinado: cada item que a entrega de uma
//! onda ou o veredito final responde em `agreed`, pelo número vigente, e o
//! item não cumprido que vira tarefa no backlog — uma vez só: a tarefa ainda
//! por entregar que já cobre o item basta, e nenhuma outra nasce, e o item
//! que a análise da onda tirou do pedido dela não vira tarefa nenhuma.

use std::collections::BTreeSet;

use mustard_core::domain::normalize::Languages;
use mustard_core::domain::spec_events::{Refusal, SpecEvent, SpecLog};
use mustard_core::domain::wave_prompt as agreed_prompt;
use serde_json::{json, Map, Value};

use super::queue::tasks_not_delivered;
use super::report::{agreed_item_id, dispatched_at};

/// O que [`settle_agreed`] devolve: a tarefa de cada item não cumprido e o
/// código de cada item esperado que ficou sem resposta.
pub(super) type SettledAgreed = (Vec<Map<String, Value>>, Vec<String>);

/// Os itens combinados que o pedido da onda `wave` levou: o que ele lê
/// ([`agreed_prompt::dispatch_items`]), só com os tipos do bloco do
/// combinado que o veredito final também responde — regra, limite, contrato,
/// erro, caso de borda, fora do escopo e decisão. A entrega da onda responde
/// por cada um deles.
///
/// A lista é a do envio que despachou a onda ([`dispatched_at`]), não a de
/// agora: a leitura é feita sobre a spec como estava nele, só com os eventos
/// de número até o dele — os números da spec só crescem. O item combinado
/// gravado entre o envio e a volta não é cobrado, porque o pedido não o
/// levou. O que o pedido levou e ganhou versão nova depois é cobrado pela
/// versão de agora, pelo mesmo código; o que saiu da spec depois, não.
pub(super) fn request_agreed<'a>(log: &'a SpecLog, wave: u64, languages: &Languages) -> Vec<&'a SpecEvent> {
    let then = as_dispatched(log, wave);
    let then_codes = then.codes();
    let carried: Vec<&String> =
        agreed_prompt::dispatch_items(&then, wave, None, languages).iter().filter_map(|item| then_codes.get(&item.id)).collect();
    let codes = log.codes();
    let agreed = agreed_prompt::all_agreed(log);
    carried
        .into_iter()
        .filter_map(|code| agreed.iter().copied().find(|item| codes.get(&item.id) == Some(code)))
        .collect()
}

/// A spec como estava no envio que despachou a onda `wave`: só os eventos de
/// número até o dele. Sem envio, a spec inteira.
pub(super) fn as_dispatched(log: &SpecLog, wave: u64) -> SpecLog {
    let sent = dispatched_at(log, wave).unwrap_or(u64::MAX);
    SpecLog { events: log.events.iter().filter(|e| e.id <= sent).cloned().collect(), ..SpecLog::default() }
}

/// Os códigos dos itens que alguma tarefa ainda por entregar cobre
/// ([`tasks_not_delivered`]), fora as das ondas em `returning`. Pelo código,
/// a tarefa que cobre uma versão antiga do item cobre também a de agora.
pub(super) fn covered_codes(log: &SpecLog, returning: &BTreeSet<u64>) -> BTreeSet<String> {
    let codes = log.codes();
    tasks_not_delivered(log, returning)
        .into_iter()
        .filter_map(|id| log.get(id))
        .flat_map(|task| task.ints("covers"))
        .filter_map(|id| codes.get(&id).cloned())
        .collect()
}

/// Os códigos dos itens do projeto todo que a análise da onda `wave` tirou do
/// pedido dela: a escolha gravada no envio que a despachou, lida sobre a spec
/// como estava nele ([`as_dispatched`]) e reduzida aos candidatos de então,
/// a mesma que monta o pedido ([`agreed_prompt::dispatch_items`]). O pedido
/// não levou esses itens: a entrega não é cobrada por eles, e a resposta que
/// der a algum deles não cria tarefa.
pub(super) fn removed_by_analysis(log: &SpecLog, wave: u64, languages: &Languages) -> BTreeSet<String> {
    let then = as_dispatched(log, wave);
    let Some(choice) = agreed_prompt::recorded_choice(&then, wave) else { return BTreeSet::new() };
    let codes = then.codes();
    let choice = choice.within(&agreed_prompt::candidates(&then, wave, languages));
    choice.removed.iter().filter_map(|(id, _)| codes.get(id).cloned()).collect()
}

/// A resposta `agreed` de uma volta — a entrega de uma onda ou o veredito
/// final — resolvida contra os itens combinados que ela precisa responder
/// (`expected`): cada item citado vira o número vigente dele, e o que não
/// vem `met:true` vira uma tarefa nova no backlog, de autor `author`,
/// cobrindo o item, com o que a resposta diz em `text` — sem ele, o texto do
/// próprio item — e os arquivos que ela cita. O item cujo código está em
/// `covered`, que alguma tarefa ainda por entregar cobre, fica na resposta
/// como veio, sem tarefa nova; cada tarefa que nasce entra em `covered`. O
/// item cujo código está em `removed`, que a análise da onda tirou do pedido
/// ([`removed_by_analysis`]), também fica como veio e não vira tarefa. Devolve essas tarefas e o código de cada item esperado que ficou sem
/// resposta. O item citado que a spec não tem é recusado.
pub(super) fn settle_agreed(
    log: &SpecLog,
    draft: &mut Map<String, Value>,
    expected: &[&SpecEvent],
    author: &str,
    covered: &mut BTreeSet<String>,
    removed: &BTreeSet<String>,
) -> Result<SettledAgreed, Refusal> {
    let (mut answered, mut tasks) = (Vec::new(), Vec::new());
    let codes = log.codes();
    for item in draft.get_mut("agreed").and_then(Value::as_array_mut).into_iter().flatten() {
        let id = agreed_item_id(log, item.get("item").unwrap_or(&Value::Null))?;
        item["item"] = json!(id);
        answered.push(id);
        if item.get("met").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let code = codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
        if removed.contains(&code) || !covered.insert(code) {
            continue;
        }
        let said = item.get("text").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
        let text = said.or_else(|| log.get(id).and_then(|e| e.str_field("text"))).unwrap_or_default();
        let files = item.get("files").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
        let files: Vec<Value> = files.map(|f| json!({ "path": f })).collect();
        let task = json!({ "text": text, "files": files, "depends_on": [], "covers": [id], "author": author });
        tasks.extend(task.as_object().cloned());
    }
    let missing = expected
        .iter()
        .filter(|item| !answered.contains(&item.id))
        .map(|item| codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string()))
        .collect();
    Ok((tasks, missing))
}
