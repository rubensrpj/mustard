//! A resposta de uma volta pelo combinado: cada item que a entrega de uma
//! onda ou o veredito final responde em `agreed`, pelo número vigente, e o
//! item não cumprido que vira trabalho no backlog — uma vez só: a tarefa
//! ainda por entregar que já cobre o item basta, e nenhuma outra nasce, e o
//! item que a análise da onda tirou do pedido dela não vira tarefa nenhuma.
//! Na volta que deixou tarefas por fazer, o item entra numa delas, na versão
//! que volta ao backlog, em vez de nascer como tarefa nova.

use std::collections::BTreeSet;

use mustard_core::domain::spec_events::{Refusal, SpecEvent, SpecLog};
use mustard_core::domain::wave_prompt as agreed_prompt;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::queue::tasks_not_delivered;
use super::report::{agreed_item_id, dispatched_at, WaveReport};
use super::stops::append_agent_line;

/// O que [`settle_agreed`] devolve: a tarefa de cada item não cumprido e o
/// código de cada item esperado que ficou sem resposta.
pub(super) type SettledAgreed = (Vec<Map<String, Value>>, Vec<String>);

/// Os itens combinados que o pedido da onda `wave` levou: a lista que o envio
/// que a despachou ([`dispatched_at`]) gravou do que o pedido lia, pela mesma
/// conta que montou "O que obedecer" ([`agreed_prompt::sent_agreed`]) — os
/// tipos do bloco do combinado que o veredito final também responde: regra,
/// limite, contrato, erro, caso de borda, fora do escopo e decisão. A entrega
/// da onda responde por cada um deles. O item que a escolha da onda tirou do
/// pedido não está na lista e não é cobrado, também depois de um reenvio. O
/// envio gravado antes da lista fica com a conta da montagem
/// ([`agreed_prompt::dispatch_items`]), com a escolha que ele gravou.
///
/// A lista é a do envio, não a de agora: a leitura é feita sobre a spec como
/// estava nele, só com os eventos de número até o dele — os números da spec
/// só crescem. O item combinado gravado entre o envio e a volta não é
/// cobrado, porque o pedido não o levou. O que o pedido levou e ganhou versão
/// nova depois é cobrado pela versão de agora, pelo mesmo código; o que saiu
/// da spec depois, não.
pub(super) fn request_agreed(log: &SpecLog, wave: u64) -> Vec<&SpecEvent> {
    let then = as_dispatched(log, wave);
    let then_codes = then.codes();
    let sent = dispatched_at(log, wave).and_then(|id| then.get(id)).and_then(|send| agreed_prompt::sent_agreed(&then, send));
    let carried = sent.unwrap_or_else(|| agreed_prompt::carried_agreed(agreed_prompt::dispatch_items(&then, wave, None)));
    let carried: Vec<&String> = carried.iter().filter_map(|item| then_codes.get(&item.id)).collect();
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

/// Os códigos dos itens que a análise da onda `wave` tirou do pedido dela, o
/// do projeto todo e o dos arquivos da onda: a escolha gravada no envio que
/// a despachou, lida sobre a spec como estava nele ([`as_dispatched`]) e
/// reduzida aos candidatos de então, a mesma que monta o pedido
/// ([`agreed_prompt::dispatch_items`]). O pedido não levou esses itens: a
/// entrega não é cobrada por eles, e a resposta que der a algum deles não
/// cria tarefa.
fn removed_by_analysis(log: &SpecLog, wave: u64) -> BTreeSet<String> {
    let then = as_dispatched(log, wave);
    let Some(choice) = agreed_prompt::recorded_choice(&then, wave) else { return BTreeSet::new() };
    let codes = then.codes();
    let choice = choice.within(&agreed_prompt::candidates(&then, wave));
    choice.removed.iter().filter_map(|(id, _)| codes.get(id).cloned()).collect()
}

/// Um item combinado que a resposta não cumpriu e que ainda pede trabalho: o
/// número vigente dele, o que a resposta diz que falta — sem isso, o texto do
/// próprio item — e os arquivos que ela cita, cada um como `{"path": …}`.
struct Unmet {
    id: u64,
    text: String,
    files: Vec<Value>,
}

/// Resolve a resposta `agreed` de `draft`: cada item citado vira o número
/// vigente dele. Devolve o número de cada item respondido e, na ordem da
/// resposta, cada item que não vem `met:true` e ainda pede trabalho: fica de
/// fora o de código em `covered`, que alguma tarefa ainda por entregar já
/// cobre, e o de código em `removed`, que a análise da onda tirou do pedido.
/// Cada item devolvido entra em `covered`. O item citado que a spec não tem é
/// recusado.
fn answered_items(
    log: &SpecLog,
    draft: &mut Map<String, Value>,
    covered: &mut BTreeSet<String>,
    removed: &BTreeSet<String>,
) -> Result<(Vec<u64>, Vec<Unmet>), Refusal> {
    let (mut answered, mut unmet) = (Vec::new(), Vec::new());
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
        let text = said.or_else(|| log.get(id).and_then(|e| e.str_field("text"))).unwrap_or_default().to_string();
        let files = item.get("files").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
        let files = files.map(|f| json!({ "path": f })).collect();
        unmet.push(Unmet { id, text, files });
    }
    Ok((answered, unmet))
}

/// A resposta `agreed` de uma volta — a entrega de uma onda ou o veredito
/// final — resolvida contra os itens combinados que ela precisa responder
/// (`expected`) por [`answered_items`]: cada item que não vem `met:true` e
/// ainda pede trabalho vira uma tarefa nova no backlog, de autor `author`,
/// cobrindo o item, com o que a resposta diz que falta e os arquivos que ela
/// cita. O item já em `covered` ou em `removed` fica na resposta como veio,
/// sem tarefa nova; cada tarefa que nasce entra em `covered`. Na volta que
/// deixou tarefas por fazer, o item não cumprido já entrou numa delas
/// ([`join_unmet`]), que o cobre, e chega aqui em `covered`. Devolve as
/// tarefas novas e o código de cada item esperado que ficou sem resposta.
pub(super) fn settle_agreed(
    log: &SpecLog,
    draft: &mut Map<String, Value>,
    expected: &[&SpecEvent],
    author: &str,
    covered: &mut BTreeSet<String>,
    removed: &BTreeSet<String>,
) -> Result<SettledAgreed, Refusal> {
    let (answered, unmet) = answered_items(log, draft, covered, removed)?;
    let tasks = unmet
        .into_iter()
        .filter_map(|Unmet { id, text, files }| {
            json!({ "text": text, "files": files, "depends_on": [], "covers": [id], "author": author })
                .as_object()
                .cloned()
        })
        .collect();
    let codes = log.codes();
    let missing = expected
        .iter()
        .filter(|item| !answered.contains(&item.id))
        .map(|item| codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string()))
        .collect();
    Ok((tasks, missing))
}

/// A resposta `agreed` da volta `wave`, já na entrega oficial `draft` dela,
/// resolvida por [`settle_agreed`]: devolve a tarefa de cada item não
/// cumprido que ainda pede trabalho. Não ganha tarefa o item que uma tarefa
/// ainda por entregar já cobre — a que a onda deixou por fazer, que o recebeu
/// ([`join_unmet`]), inclusive —, o que uma tarefa nascida antes na mesma
/// rodada cobre (`covered_now`) e o que a análise da onda tirou do pedido. As
/// tarefas da onda que volta e das que o conserto dela fecha não contam: a
/// entrega as fecha agora. Na volta cujas tarefas por fazer outra onda já
/// levou, todas, nenhum item ganha tarefa: ele está no pedido da onda que
/// levou a tarefa, que responde por ele. Cada item que ganha tarefa entra em
/// `covered_now`.
pub(super) fn settle_wave_agreed(
    log: &SpecLog,
    wave: &WaveReport,
    draft: &mut Map<String, Value>,
    covered_now: &mut BTreeSet<String>,
) -> Result<Vec<Map<String, Value>>, Refusal> {
    let returning: BTreeSet<u64> = std::iter::once(wave.wave).chain(wave.fixes.iter().copied()).collect();
    let mut covered = covered_codes(log, &returning);
    covered.extend(covered_now.iter().cloned());
    let known = covered.clone();
    let (tasks, _) = settle_agreed(log, draft, &[], "wave", &mut covered, &removed_by_analysis(log, wave.wave))?;
    if wave.taken_elsewhere && wave.undone.is_empty() {
        return Ok(Vec::new());
    }
    covered_now.extend(covered.difference(&known).cloned());
    Ok(tasks)
}

/// Junta o item combinado que cada volta de `waves` não cumpriu à tarefa que
/// ela deixou por fazer, em vez de deixá-lo virar tarefa nova: `returned` traz
/// a versão que volta ao backlog de cada tarefa não feita, com a onda que a
/// devolveu, na ordem de `undone`. O item vai à tarefa que nasceu dele
/// (`origin`), a ligação que o pedido da onda mostra; sem ela, à primeira da
/// lista. A versão passa a cobrir o item, ganha os arquivos que a resposta
/// cita e, na parte do agente, uma linha com o que falta, no idioma `lang`.
/// Fica de fora, como em [`settle_agreed`], o item que outra tarefa ainda por
/// entregar ou a própria tarefa devolvida já cobre e o que a análise da onda
/// tirou do pedido; o item que uma volta já juntou não entra de novo pela
/// seguinte. A volta que fez todas as tarefas não junta nada, nem a que
/// deixou por fazer só tarefas que outra onda já levou: nenhuma delas volta
/// ao backlog.
pub(super) fn join_unmet(
    log: &SpecLog,
    waves: &[WaveReport],
    returned: &mut [(u64, Map<String, Value>)],
    lang: Locale,
) -> Result<(), Refusal> {
    let codes = log.codes();
    let code_of = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
    let ids = |task: &Map<String, Value>, key: &str| -> Vec<u64> {
        let many = task.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64);
        many.chain(task.get(key).and_then(Value::as_u64)).collect()
    };
    let mut joined: BTreeSet<String> = BTreeSet::new();
    for wave in waves.iter().filter(|wave| !wave.agreed.is_empty()) {
        let own: Vec<usize> = (0..returned.len()).filter(|at| returned[*at].0 == wave.wave).collect();
        let Some(&first) = own.first() else { continue };
        let returning: BTreeSet<u64> = std::iter::once(wave.wave).chain(wave.fixes.iter().copied()).collect();
        let mut covered = covered_codes(log, &returning);
        covered.extend(joined.iter().cloned());
        covered.extend(own.iter().flat_map(|at| ids(&returned[*at].1, "covers")).map(code_of));
        let mut answer = Map::from_iter([("agreed".to_string(), json!(wave.agreed))]);
        let (_, unmet) = answered_items(log, &mut answer, &mut covered, &removed_by_analysis(log, wave.wave))?;
        for Unmet { id, text, files } in unmet {
            let code = code_of(id);
            let born_of = |at: &usize| ids(&returned[*at].1, "origin").into_iter().any(|origin| code_of(origin) == code);
            let task = &mut returned[own.iter().copied().find(born_of).unwrap_or(first)].1;
            let mut covers = ids(task, "covers");
            covers.push(id);
            task.insert("covers".into(), json!(covers));
            let mut paths: Vec<Value> = task.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
            for file in files {
                if !paths.contains(&file) {
                    paths.push(file);
                }
            }
            task.insert("files".into(), json!(paths));
            let said = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let line = translate("round.unmet_joined", lang)
                .replace("{wave}", &wave.wave.to_string())
                .replace("{code}", &code)
                .replace("{text}", &said);
            append_agent_line(task, &line);
            joined.insert(code);
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use std::path::Path;

    use mustard_core::io::spec_events as store;
    use mustard_core::io::wave_prompt::shown;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::super::tests::{approved_with, returned, round, waves_in, write};
    use super::{agreed_prompt, request_agreed};
    use crate::commands::spec_events::read::read_by_command;

    /// A spec `x` com a onda 1 sobre `src/app.ts` e duas regras do projeto
    /// todo; o Jev tira a segunda do pedido, e a onda sai. Devolve a resposta
    /// da rodada que a despachou.
    pub(in crate::commands::flow::round) fn sent_without_the_second_rule(root: &Path) -> Value {
        approved_with(root, "x", &[(1, &["src/app.ts"], &[])], |said| {
            for text in ["Vale sempre: a tabela nova tem chave.", "Vale sempre: a spec vira um PR só."] {
                let rule = json!({"title": text, "text": text, "example": "e", "keys": ["k"],
                    "applies_to": {"files": ["**"]}, "origin": said});
                assert_eq!(write(root, "x", "rule", rule)["ok"], json!(true));
            }
        });
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let second = log.codes().into_iter().find(|(_, code)| code == "MSTD-RULE-0002").map(|(id, _)| id).expect("the second rule");
        let _jev = super::super::item_choice::fake::answering(move |board| {
            board.items.iter().map(|item| (item.id, if item.id == second { 0.0 } else { 1.0 })).collect()
        });
        let sent = round(root, "x", None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
        sent
    }

    /// O comando que lê o bloco `block` da spec `x` do projeto em `root`.
    pub(in crate::commands::flow::round) fn read_block(root: &Path, block: &str) -> String {
        read_by_command(&format!("mustard-rt run read {block} --root {} --spec x", shown(root)))
    }

    /// O pedido mostra e a entrega cobra os mesmos itens combinados: os da
    /// lista que o envio da onda gravou. Depois de um reenvio gravado com a
    /// lista e o pedido do envio anterior, mas sem a escolha que tirou a
    /// segunda regra — como os reenvios ficaram gravados —, o pedido não a
    /// mostra, a entrega não é cobrada por ela e grava sem ela.
    #[test]
    fn the_request_and_its_delivery_carry_the_agreed_items_its_send_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        sent_without_the_second_rule(root);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let prior = log.visible().into_iter().rfind(|e| e.event_type == "send" && e.wave() == Some(1)).cloned().unwrap();
        assert!(prior.fields.get("analysis").is_some(), "{prior:?}");
        let mut resent = serde_json::Map::new();
        for field in ["wave", "role", "text", "lines", "chars", "items", "read_items", "mustard", "copy", "author"] {
            if let Some(value) = prior.fields.get(field) {
                resent.insert(field.into(), value.clone());
            }
        }
        resent.insert("resends".into(), json!(prior.id));
        store::write(&path, "send", resent, &[]).unwrap();

        let log = store::read(&path).unwrap().unwrap();
        let codes = log.codes();
        let charged: Vec<String> = request_agreed(&log, 1).iter().map(|item| codes[&item.id].clone()).collect();
        assert_eq!(charged, ["MSTD-RULE-0001"], "only what the request carried");
        let request = read_block(root, "request-1");
        for item in agreed_prompt::all_agreed(&log) {
            let code = &codes[&item.id];
            assert_eq!(request.contains(code.as_str()), charged.contains(code), "{code}, side by side: {request}");
        }
        let wrote = returned(root, json!({"wave": 1, "text": "A onda 1 saiu.", "agreed": [{"item": "MSTD-RULE-0001", "met": true}]}));
        assert_eq!(wrote["ok"], json!(true), "the removed rule is not charged: {wrote}");
    }
}
