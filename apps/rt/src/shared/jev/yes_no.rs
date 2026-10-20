//! A pergunta de sim ou não ao Jev sobre várias entradas de uma vez: uma
//! pergunta `noul` por entrada, todas sobre o mesmo estado, numa chamada. O
//! estado que passa do que um pedido aceita sai em partes, cada uma com o
//! mesmo contexto e um pedaço das entradas, e as partes vão ao mesmo tempo.
//! A resposta é a chance de sim de cada entrada; quem pergunta decide o que
//! fazer com ela. Sem resposta, ou com uma chance que falta ou não se lê,
//! nada volta, e quem pergunta segue sem o Jev.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value, json};

use super::{JEV_MODEL, JevFilter};

/// O tamanho de cada parte, em caracteres: o contexto, as entradas e as
/// perguntas dela. Pequeno de propósito, uns 2 mil tokens: a documentação do
/// Jev diz que o acerto cai quando o estado cresce com o que não importa à
/// pergunta, e o custo quase não muda, porque cada entrada vai uma vez só.
const PART_CHARS: usize = 8_000;

/// Uma pergunta de sim ou não feita a cada entrada de uma parte do estado.
pub(crate) struct YesNo<'a> {
    /// O estado comum a todas as perguntas, sem as entradas. Um objeto com o
    /// nome de `section` vai junto das entradas, na mesma parte do estado.
    pub(crate) context: Map<String, Value>,
    /// O nome da parte do estado que leva as entradas.
    pub(crate) section: &'a str,
    /// A pergunta, com `{entry}` no lugar da entrada (`<section>.<chave>`).
    pub(crate) question: &'a str,
    /// O recorte da resposta.
    pub(crate) focus: &'a str,
    /// O que é sim.
    pub(crate) yes: &'a str,
    /// O que é não.
    pub(crate) no: &'a str,
}

impl YesNo<'_> {
    /// A pergunta sobre a entrada `key`.
    fn question_for(&self, key: &str) -> Value {
        json!({
            "type": "noul",
            "instructions": {
                "question": self.question.replace("{entry}", &format!("{}.{key}", self.section)),
                "focus": self.focus,
            },
            "criteria": { "true": { "what": self.yes }, "false": { "what": self.no } },
        })
    }

    /// Os pedidos para as entradas `entries` (a chave e o que o estado mostra
    /// dela), cada um com as chaves que pergunta, na ordem das entradas. Sem
    /// entrada, nenhum.
    pub(crate) fn payloads(&self, entries: &[(String, Value)]) -> Vec<(String, Vec<String>)> {
        let context_chars = Value::Object(self.context.clone()).to_string().chars().count();
        let mut parts: Vec<Vec<usize>> = Vec::new();
        let mut used = context_chars;
        for (at, (key, shown)) in entries.iter().enumerate() {
            let chars = shown.to_string().chars().count() + self.question_for(key).to_string().chars().count() + 2 * key.chars().count();
            match parts.last_mut() {
                Some(part) if used + chars <= PART_CHARS => part.push(at),
                _ => {
                    parts.push(vec![at]);
                    used = context_chars;
                }
            }
            used += chars;
        }
        parts
            .into_iter()
            .map(|part| {
                let mut state = self.context.clone();
                // O contexto pode trazer, na mesma parte das entradas, o que
                // cada pergunta compara com elas.
                let mut shown = state.get(self.section).and_then(Value::as_object).cloned().unwrap_or_default();
                shown.extend(part.iter().map(|at| (entries[*at].0.clone(), entries[*at].1.clone())));
                state.insert(self.section.to_string(), Value::Object(shown));
                let questions: Map<String, Value> = part.iter().map(|at| (entries[*at].0.clone(), self.question_for(&entries[*at].0))).collect();
                let payload = json!({ "model": JEV_MODEL, "state": state, "questions": questions }).to_string();
                (payload, part.iter().map(|at| entries[*at].0.clone()).collect())
            })
            .collect()
    }
}

/// A certeza alta, a única em que quem pergunta age sozinho: a chance de sim
/// a partir dela, a de não até `1 - SURE`, e a confiança da escolha a partir
/// dela. Abaixo disso a decisão volta a quem conduz, ou à regra de sempre.
pub(crate) const SURE: f64 = 0.8;

/// A chance de sim da pergunta `key` na resposta `doc`, quando é um número
/// entre 0 e 1.
pub(crate) fn noul_in(doc: &Value, key: &str) -> Option<f64> {
    doc.pointer(&format!("/answers/{key}/noul")).and_then(Value::as_f64).filter(|chance| (0.0..=1.0).contains(chance))
}

/// A opção da pergunta de escolha `key` na resposta `doc`, com a confiança
/// dela, quando a opção é uma de `allowed` e a confiança é um número entre 0
/// e 1. A confiança diz quanto a resposta se concentrou numa opção só.
pub(crate) fn choice_in(doc: &Value, key: &str, allowed: &[&str]) -> Option<(String, f64)> {
    let answer = doc.pointer(&format!("/answers/{key}"))?;
    let choice = answer["choice"].as_str().filter(|choice| allowed.contains(choice))?;
    let confidence = answer["confidence"].as_f64().filter(|confidence| (0.0..=1.0).contains(confidence))?;
    Some((choice.to_string(), confidence))
}

/// A chance de sim de cada chave dos pedidos `parts`, lida das respostas
/// `docs`, na mesma ordem. `None` quando falta resposta, ou chance de alguma
/// chave, ou ela não é um número entre 0 e 1.
pub(crate) fn chances(parts: &[(String, Vec<String>)], docs: &[Value]) -> Option<BTreeMap<String, f64>> {
    if parts.len() != docs.len() {
        return None;
    }
    let mut out = BTreeMap::new();
    for ((_, keys), doc) in parts.iter().zip(docs) {
        for key in keys {
            let chance = doc.pointer(&format!("/answers/{key}/noul")).and_then(Value::as_f64).filter(|chance| (0.0..=1.0).contains(chance))?;
            out.insert(key.clone(), chance);
        }
    }
    Some(out)
}

/// As perguntas de cada grupo de `groups` (a pergunta e as entradas dela),
/// todas pedidas por `invoke` de uma vez, sem reserva: a forma dos testes.
/// Volta, por grupo e na ordem, a chance de sim de cada entrada; `None` no
/// grupo cuja resposta falta ou não se lê, e em todos quando a chamada
/// falha. Sem entrada nenhuma, nada sai.
#[cfg(test)]
pub(crate) fn ask_all(groups: &[(&YesNo<'_>, &[(String, Value)])], invoke: &dyn Fn(&[String]) -> Option<Vec<Value>>) -> Vec<Option<BTreeMap<String, f64>>> {
    let parts: Vec<Vec<(String, Vec<String>)>> = groups.iter().map(|(question, entries)| question.payloads(entries)).collect();
    let payloads: Vec<String> = parts.iter().flatten().map(|(payload, _)| payload.clone()).collect();
    if payloads.is_empty() {
        return parts.iter().map(|_| Some(BTreeMap::new())).collect();
    }
    let Some(docs) = invoke(&payloads).filter(|docs| docs.len() == payloads.len()) else {
        return parts.iter().map(|_| None).collect();
    };
    let mut at = 0;
    parts
        .iter()
        .map(|group| {
            let answered = &docs[at..at + group.len()];
            at += group.len();
            chances(group, answered)
        })
        .collect()
}

/// O que as perguntas de sim ou não deram, com a reserva do Jev.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Decided {
    /// A chance de sim de cada entrada, por grupo, na ordem dos grupos: do
    /// Jev ou do recibo da reserva.
    Chances(Vec<Option<BTreeMap<String, f64>>>),
    /// O Jev está desligado pelo usuário: vale o fluxo sem Jev, sem reserva.
    Off,
    /// A decisão está na reserva, à espera do recibo: o pedido pronto ao
    /// agente de análise da sessão.
    Reserve(Value),
}

/// As perguntas de cada grupo de `groups`, todas pedidas ao Jev `jev` de uma
/// vez, com a reserva dele ([`super::reserve`]), com as chamadas
/// gravadas como `command` na spec `spec` de `root`. O recibo da reserva já
/// entregue desta mesma decisão responde no lugar do Jev, sem chamá-lo. O
/// Jev ligado (`jev`) que falha ou volta ilegível manda a decisão à reserva,
/// a mesma a cada passagem até o recibo; a reserva que não se prepara deixa a
/// resposta como veio. Com o Jev desligado, nada vai à reserva.
pub(crate) fn ask_all_reserved(root: &Path, spec: &str, command: &str, groups: &[(&YesNo<'_>, &[(String, Value)])], jev: Option<&AskFn<'_>>) -> Decided {
    let Some(invoke) = jev else { return Decided::Off };
    let parts: Vec<Vec<(String, Vec<String>)>> = groups.iter().map(|(question, entries)| question.payloads(entries)).collect();
    let payloads: Vec<String> = parts.iter().flatten().map(|(payload, _)| payload.clone()).collect();
    if payloads.is_empty() {
        return Decided::Chances(parts.iter().map(|_| Some(BTreeMap::new())).collect());
    }
    let read = |docs: &[Value]| -> Vec<Option<BTreeMap<String, f64>>> {
        let mut at = 0;
        parts
            .iter()
            .map(|group| {
                let answered = &docs[at..at + group.len()];
                at += group.len();
                chances(group, answered)
            })
            .collect()
    };
    if let Some(docs) = super::reserve::receipt_for(root, &payloads).filter(|docs| docs.len() == payloads.len()) {
        return Decided::Chances(read(&docs));
    }
    let got = invoke(&payloads).filter(|docs| docs.len() == payloads.len()).map(|docs| read(&docs));
    if let Some(got) = &got
        && got.iter().all(Option::is_some)
    {
        return Decided::Chances(got.clone());
    }
    let request = super::reserve::reserve_request(root, spec, command, &payloads);
    if request.get("unavailable").is_none() {
        return Decided::Reserve(request);
    }
    Decided::Chances(got.unwrap_or_else(|| parts.iter().map(|_| None).collect()))
}

/// [`ask_all`] para uma pergunta só.
#[cfg(test)]
pub(crate) fn ask(question: &YesNo<'_>, entries: &[(String, Value)], invoke: &dyn Fn(&[String]) -> Option<Vec<Value>>) -> Option<BTreeMap<String, f64>> {
    ask_all(&[(question, entries)], invoke).into_iter().next().flatten()
}

/// A pergunta do resumo do projeto ao Jev: recebe os pedidos e devolve as
/// respostas, na ordem, ou `None` na falha.
pub(crate) type AskFn<'a> = dyn Fn(&[String]) -> Option<Vec<Value>> + 'a;

/// [`AskFn`] guardada, com o tempo de vida do que ela usa.
pub(crate) type Ask<'a> = Box<AskFn<'a>>;

/// A pergunta do resumo do projeto `root` ao Jev, com as chamadas gravadas
/// como `command` na spec `spec` (vazia, nas chamadas soltas da máquina):
/// todos os pedidos saem ao mesmo tempo, esperados pelo prazo de toda
/// chamada. `None` quando o Jev está desligado: o interruptor geral fora ou
/// sem chave. A falha de uma chamada é a resposta `None` da pergunta.
pub(crate) fn project_jev<'a>(root: &'a Path, spec: &'a str, command: &'a str) -> Option<Ask<'a>> {
    #[cfg(test)]
    if let Some(fake) = ASK_IN_TEST.with(|slot| slot.borrow().clone()) {
        return Some(Box::new(move |payloads: &[String]| fake(payloads)));
    }
    let filter = JevFilter::for_project(root, spec)?;
    Some(Box::new(move |payloads: &[String]| filter.choose_all_recorded_within(root, spec, command, payloads, filter.timeouts.response)))
}

/// A pergunta da orquestração da spec `spec` de `root` ao Jev, com as
/// chamadas gravadas como `command`: o filtro das escolhas da orquestração
/// ([`JevFilter::for_orchestration`]), esperado pelo prazo de toda chamada.
/// `None` quando ele está desligado; a falha de uma chamada é a resposta
/// `None` da pergunta.
pub(crate) fn orchestration_jev<'a>(root: &'a Path, spec: &'a str, command: &'a str) -> Option<Ask<'a>> {
    #[cfg(test)]
    if let Some(fake) = ASK_IN_TEST.with(|slot| slot.borrow().clone()) {
        return Some(Box::new(move |payloads: &[String]| fake(payloads)));
    }
    let filter = JevFilter::for_orchestration(root, spec)?;
    Some(Box::new(move |payloads: &[String]| filter.choose_all_recorded_within(root, spec, command, payloads, filter.timeouts.response)))
}

/// O Jev que um teste põe no lugar do serviço.
#[cfg(test)]
pub(crate) type FakeAsk = std::rc::Rc<dyn Fn(&[String]) -> Option<Vec<Value>>>;

#[cfg(test)]
thread_local! {
    /// O Jev do resumo do projeto que um teste põe no lugar do serviço, só
    /// nesta linha de execução.
    static ASK_IN_TEST: std::cell::RefCell<Option<FakeAsk>> = const { std::cell::RefCell::new(None) };
}

/// Põe `fake` no lugar do Jev do resumo do projeto, nesta linha de execução;
/// `None` volta ao de todo teste, desligado.
#[cfg(test)]
pub(crate) fn ask_in_test(fake: Option<FakeAsk>) {
    ASK_IN_TEST.with(|slot| *slot.borrow_mut() = fake);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question() -> YesNo<'static> {
        let mut context = Map::new();
        context.insert("goal".into(), json!("Resumo do projeto."));
        YesNo { context, section: "entries", question: "Is `{entry}` related to `goal`?", focus: "Only the goal.", yes: "Related", no: "Unrelated" }
    }

    /// Uma pergunta `noul` por entrada, no mesmo estado, com o contexto e a
    /// entrada nomeada na pergunta.
    #[test]
    fn one_noul_question_per_entry_over_the_same_state() {
        let entries = vec![("e1".to_string(), json!({"title":"A"})), ("e2".to_string(), json!({"title":"B"}))];
        let parts = question().payloads(&entries);
        assert_eq!(parts.len(), 1);
        let body: Value = serde_json::from_str(&parts[0].0).unwrap();
        assert_eq!(body["model"], JEV_MODEL);
        assert_eq!(body["state"], json!({"goal":"Resumo do projeto.","entries":{"e1":{"title":"A"},"e2":{"title":"B"}}}));
        assert_eq!(body["questions"]["e2"]["type"], "noul");
        assert_eq!(body["questions"]["e2"]["instructions"]["question"], "Is `entries.e2` related to `goal`?");
        assert_eq!(parts[0].1, vec!["e1", "e2"]);
        assert!(question().payloads(&[]).is_empty());
    }

    /// O estado grande sai em partes que cabem num pedido, e as respostas
    /// voltam juntas, pela chave.
    /// Cada pergunta vê pouco material além da entrada dela: o acerto do Jev
    /// cai com estado grande e cheio do que não importa à pergunta.
    #[test]
    fn each_part_carries_little_besides_its_own_entries() {
        let entries: Vec<(String, Value)> = (1..=20).map(|n| (format!("e{n}"), json!({"text": "x".repeat(1_500)}))).collect();
        let parts = question().payloads(&entries);
        assert!(parts.len() >= 4, "{} parts", parts.len());
        for (payload, keys) in &parts {
            let body: Value = serde_json::from_str(payload).unwrap();
            assert!(body["state"].to_string().chars().count() <= 8_000, "{} entries in one state", keys.len());
        }
    }

    #[test]
    fn a_big_state_goes_in_parts_and_the_answers_come_back_by_key() {
        let entries: Vec<(String, Value)> = (1..=40).map(|n| (format!("e{n}"), json!({"text": "x".repeat(4_000)}))).collect();
        let parts = question().payloads(&entries);
        assert!(parts.len() > 1, "{}", parts.len());
        for (payload, _) in &parts {
            assert!(super::super::validate_wire_size(payload).is_ok());
        }
        let answer = |payload: &String| {
            let body: Value = serde_json::from_str(payload).unwrap();
            let answers: Map<String, Value> =
                body["questions"].as_object().unwrap().keys().map(|key| (key.clone(), json!({"type":"noul","noul":if key == "e7" { 0.9 } else { 0.1 }}))).collect();
            json!({ "answers": answers })
        };
        let got = ask(&question(), &entries, &|payloads| Some(payloads.iter().map(answer).collect())).unwrap();
        assert_eq!(got.len(), 40);
        assert_eq!(got["e7"], 0.9);
        assert_eq!(got["e8"], 0.1);
    }

    /// Sem resposta, ou com uma chance que falta, nada volta; o grupo sem
    /// entrada não chama nada.
    #[test]
    fn a_missing_answer_gives_nothing_and_no_entry_asks_nothing() {
        let entries = vec![("e1".to_string(), json!({})), ("e2".to_string(), json!({}))];
        assert_eq!(ask(&question(), &entries, &|_| None), None);
        assert_eq!(ask(&question(), &entries, &|_| Some(vec![json!({"answers":{"e1":{"noul":0.9}}})])), None);
        assert_eq!(ask(&question(), &entries, &|_| Some(vec![json!({"answers":{"e1":{"noul":0.9},"e2":{"noul":1.5}}})])), None);
        let called = std::cell::Cell::new(false);
        assert_eq!(ask(&question(), &[], &|_| { called.set(true); None }), Some(BTreeMap::new()));
        assert!(!called.get());
    }
}
