//! A reserva do Jev. Quando uma pergunta ao Jev ligado falha, passa do
//! prazo dela ou volta ilegível, quem perguntou recebe "reserva necessária"
//! ([`Asked::Failed`]) e entrega a mesma decisão — o mesmo material e as
//! mesmas perguntas, em texto curto e literal — ao agente de análise da
//! própria sessão (o Haiku no Claude, a Luna no Codex, ou o modelo da família
//! que o projeto escolhe em `ai.session_models`), pela porta de duas fases do
//! conhecimento ([`reserve_request`]). O agente responde, por pergunta, a
//! opção escolhida e `sure` ou `unsure`; o recibo volta por
//! `run knowledge --complete-session`, e quem perguntou o lê como a resposta
//! do Jev ([`reserved`]): `sure` vale como certeza alta, acima do limiar de
//! 0,8, e `unsure` vale como dúvida. Até o recibo, a decisão fica pendente.
//! Cada reserva entregue fica no arquivo de operações da spec como chamada de
//! reserva ([`RESERVE_CALL`]). Com o Jev desligado pelo usuário
//! ([`Asked::Off`]), nada vai à reserva: vale o fluxo sem Jev.

use std::path::Path;

use mustard_core::io::knowledge::enrichment::{ReserveAnswer, ReserveOption, ReserveQuestion, completed_reserve, prepare_reserve, prepared_reserve};
use mustard_core::io::sha256::Sha256;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use super::JevFilter;
pub(crate) use super::call_record::record_reserve_call;
use crate::shared::secret::{value_without_secrets, without_secrets};
use crate::shared::spec_state::{is_codex_session, session_from_env};

pub(crate) use super::call_record::RESERVE_CALL;

/// O campo com que quem conduz repete o passo que pediu a reserva, com o
/// número do pedido dela.
pub(crate) const RESERVE_FIELD: &str = "reserve";

/// A chance e a confiança que a resposta certa da reserva vale: acima do
/// limiar de certeza de toda decisão (0,8).
const SURE_CHANCE: f64 = 0.9;

/// A confiança que a resposta incerta da reserva vale: abaixo do limiar de
/// certeza e do piso que sobe o modelo da onda um degrau (0,5).
const UNSURE_CHANCE: f64 = 0.4;

/// A chance de sim que o sim ou não incerto vale: o meio, nem sim nem não.
const DOUBT: f64 = 0.5;

/// O que a pergunta ao Jev deu.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Asked<T> {
    /// O Jev respondeu.
    Answered(T),
    /// O Jev está desligado pelo usuário — o interruptor geral fora, a
    /// finalidade fora ou sem chave —: vale o fluxo sem Jev, sem reserva.
    Off,
    /// O Jev ligado falhou, passou do prazo dele ou voltou ilegível: a
    /// decisão vai à reserva.
    Failed,
}

/// `true` quando o Jev da orquestração do projeto `root` está ligado: o
/// interruptor geral, a finalidade e a chave.
pub(crate) fn jev_on(root: &Path) -> bool {
    JevFilter::for_orchestration(root, "").is_some()
}

/// A pergunta `payload` ao Jev da orquestração da spec `spec` de `root`,
/// gravada como `command`, esperada pelo prazo de toda chamada (10 s,
/// repetições incluídas).
pub(crate) fn ask_orchestration(root: &Path, spec: &str, command: &str, payload: &str) -> Asked<Value> {
    asked(JevFilter::for_orchestration(root, spec).as_ref(), root, spec, command, payload)
}

/// [`ask_orchestration`] com o filtro `filter`; sem ele, o Jev está
/// desligado.
pub(crate) fn asked(filter: Option<&JevFilter>, root: &Path, spec: &str, command: &str, payload: &str) -> Asked<Value> {
    let Some(filter) = filter else {
        return Asked::Off;
    };
    filter.choose_recorded(root, spec, command, payload).map_or(Asked::Failed, Asked::Answered)
}

/// Uma pergunta da decisão, na ordem em que vai à reserva: o pedido de que
/// ela vem, a chave dela no pedido e o tipo da resposta do Jev.
struct Asking {
    at: usize,
    key: String,
    kind: String,
}

/// O texto de um critério do Jev: a frase, ou o `what` dela; vazio na
/// pergunta sem critério.
fn meaning(criterion: &Value) -> String {
    if criterion.is_null() {
        return String::new();
    }
    criterion.as_str().or_else(|| criterion["what"].as_str()).map_or_else(|| criterion.to_string(), str::to_string)
}

/// A decisão dos pedidos ao Jev `payloads`, em texto para o agente de
/// análise: o material (o estado de cada pedido, sem segredo) e as
/// perguntas, cada uma com as opções, na ordem dos pedidos e das perguntas de
/// cada um. Com mais de um pedido, o estado de cada um vai em
/// `decision_<n>`, e a pergunta diz de qual.
fn decision(payloads: &[String]) -> (String, Vec<ReserveQuestion>, Vec<Asking>) {
    let docs: Vec<Value> = payloads.iter().map(|payload| serde_json::from_str(payload).unwrap_or(Value::Null)).collect();
    let several = docs.len() > 1;
    let material = if several {
        Value::Object(docs.iter().enumerate().map(|(at, doc)| (format!("decision_{}", at + 1), value_without_secrets(&doc["state"]))).collect())
    } else {
        docs.first().map_or(Value::Null, |doc| value_without_secrets(&doc["state"]))
    };
    let option = |name: &str, said: &Value| ReserveOption { name: name.to_string(), meaning: without_secrets(&meaning(said)) };
    let (mut questions, mut asking) = (Vec::new(), Vec::new());
    for (at, doc) in docs.iter().enumerate() {
        for (key, question) in doc["questions"].as_object().into_iter().flatten() {
            let kind = question["type"].as_str().unwrap_or_default();
            let criteria = &question["criteria"];
            let options: Vec<ReserveOption> = match kind {
                "noul" => vec![option("yes", &criteria["true"]), option("no", &criteria["false"])],
                "choice" => criteria.as_object().into_iter().flatten().map(|(name, said)| option(name, said)).collect(),
                "score" => criteria.as_array().into_iter().flatten().enumerate().map(|(level, said)| option(&level.to_string(), said)).collect(),
                _ => continue,
            };
            // A instrução vem com as partes dela, ou numa frase só.
            let instructions = &question["instructions"];
            let said: Vec<&str> = match instructions.as_str() {
                Some(whole) => vec![whole],
                None => ["question", "focus", "rule"].iter().filter_map(|part| instructions[*part].as_str()).collect(),
            };
            let text = if several { format!("In `decision_{}`: {}", at + 1, said.join(" ")) } else { said.join(" ") };
            questions.push(ReserveQuestion { id: format!("q{}", questions.len() + 1), text: without_secrets(&text), options });
            asking.push(Asking { at, key: key.clone(), kind: kind.to_string() });
        }
    }
    (material.to_string(), questions, asking)
}

/// O nome da decisão dos pedidos `payloads`: a impressão deles, pela qual a
/// resposta da reserva se acha de novo.
pub(crate) fn subject(payloads: &[String]) -> String {
    let mut hash = Sha256::new();
    for payload in payloads {
        hash.update(payload.as_bytes());
        hash.update(&[0]);
    }
    hash.hex_digest()
}

/// O pedido pronto ao agente de análise da própria sessão com a decisão dos
/// pedidos ao Jev `payloads`, que a chamada `command` da spec `spec` de
/// `root` fazia: o mesmo material e as mesmas perguntas. A reserva entregue
/// pela primeira vez fica gravada como chamada de reserva. A preparação que
/// falha dá só o motivo, em `unavailable`.
pub(crate) fn reserve_request(root: &Path, spec: &str, command: &str, payloads: &[String]) -> Value {
    let (material, questions, _) = decision(payloads);
    let (host, model) = session_agent(root);
    match prepare_reserve(root, host, model.as_deref(), &material, &questions, &subject(payloads)) {
        Ok(prepared) => {
            if prepared["repeated"] != json!(true) {
                record_reserve_call(root, spec, command);
            }
            prepared
        }
        Err(reason) => json!({ "unavailable": reason }),
    }
}

/// O anfitrião da sessão e o modelo que o projeto `root` escolhe para o
/// agente de análise dela.
fn session_agent(root: &Path) -> (&'static str, Option<String>) {
    let host = if is_codex_session(session_from_env().as_deref()) { "codex" } else { "claude" };
    let config = mustard_core::domain::config::ProjectConfig::load(root);
    (host, config.ai.session_models.get(host).cloned())
}

/// O recibo da reserva já entregue da decisão dos pedidos `payloads` de
/// `root`, achado pela própria decisão: as respostas na forma das do Jev, uma
/// por pedido. `None` sem reserva entregue ou sem recibo ainda.
pub(crate) fn receipt_for(root: &Path, payloads: &[String]) -> Option<Vec<Value>> {
    let (material, questions, _) = decision(payloads);
    let (host, model) = session_agent(root);
    let id = prepared_reserve(root, host, model.as_deref(), &material, &questions, &subject(payloads))?;
    match reserved(root, &id, payloads) {
        Reserved::Ready(docs) => Some(docs),
        Reserved::Pending | Reserved::Stale => None,
    }
}

/// O que quem conduz faz com os pedidos de reserva de uma passagem, no
/// idioma `lang`: lança o agente de análise com cada um, manda os recibos e
/// roda `command` de novo, quando o recibo decide no lugar do Jev.
pub(crate) fn hint(command: &str, lang: Locale) -> String {
    translate("triage.reserve_pass", lang).replace("{command}", command)
}

/// Põe na resposta `out` de uma passagem os pedidos de reserva `requests`,
/// em `reserves`, e em `reserves_hint` o que quem conduz faz com eles
/// ([`hint`]). Sem pedido, a resposta não muda.
pub(crate) fn attach(out: &mut Value, requests: Vec<Value>, command: &str, lang: Locale) {
    if requests.is_empty() {
        return;
    }
    match out.get_mut("reserves").and_then(Value::as_array_mut) {
        Some(list) => list.extend(requests),
        None => out["reserves"] = Value::Array(requests),
    }
    out["reserves_hint"] = json!(hint(command, lang));
}

/// Para os testes: o agente de análise responde a reserva `request` de
/// `root`, com a opção que `choose` dá ao texto de cada pergunta e a certeza
/// `certainty`, e o recibo volta pela porta do `run knowledge`.
#[cfg(test)]
pub(crate) fn answer_in_test(root: &Path, request: &Value, choose: impl Fn(&str) -> String, certainty: &str) {
    let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap_or_default();
    let count = request["questions"].as_u64().unwrap_or_default();
    let answers: Map<String, Value> = (1..=count)
        .map(|at| {
            let id = format!("q{at}");
            let text = prompt.lines().find_map(|line| line.strip_prefix(&format!("{id}: "))).unwrap_or_default();
            (id, json!({ "choice": choose(text), "certainty": certainty }))
        })
        .collect();
    let receipt = mustard_core::io::knowledge::enrichment::SessionReceipt {
        request_id: request["request_id"].as_str().unwrap_or_default().to_string(),
        model: "inherit".into(),
        effort: None,
        result: json!({ "answers": answers }),
        input_tokens: None,
        output_tokens: None,
    };
    mustard_core::io::knowledge::enrichment::complete_session(root, root, &receipt).unwrap();
}

/// O que a reserva deu para uma decisão.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reserved {
    /// As respostas, na forma da resposta do Jev, uma por pedido, na ordem.
    Ready(Vec<Value>),
    /// O recibo ainda não voltou: a decisão segue pendente.
    Pending,
    /// O recibo é de outra decisão, ou de uma que mudou desde o pedido.
    Stale,
}

/// A resposta `answer` da reserva a uma pergunta de tipo `kind`, na forma da
/// resposta do Jev: `sure` com a chance e a confiança acima do limiar de
/// certeza, `unsure` com elas abaixo, e o sim ou não incerto no meio.
pub(crate) fn as_jev(kind: &str, answer: &ReserveAnswer) -> Value {
    let chance = if answer.sure() { SURE_CHANCE } else { UNSURE_CHANCE };
    match kind {
        "noul" => {
            let yes = match (answer.sure(), answer.choice.as_str()) {
                (false, _) => DOUBT,
                (true, "yes") => SURE_CHANCE,
                (true, _) => 1.0 - SURE_CHANCE,
            };
            json!({ "type": "noul", "noul": yes })
        }
        "score" => {
            let level = answer.choice.parse::<u64>().unwrap_or_default();
            json!({ "type": "score", "score": level, "confidence": chance, "probabilities": { answer.choice.as_str(): chance } })
        }
        _ => json!({ "type": "choice", "choice": answer.choice, "confidence": chance, "probabilities": { answer.choice.as_str(): chance } }),
    }
}

/// O que a reserva `request_id` deu para a decisão dos pedidos `payloads`,
/// pedida com eles mesmos ([`reserved_as`]).
pub(crate) fn reserved(root: &Path, request_id: &str, payloads: &[String]) -> Reserved {
    reserved_as(root, request_id, &subject(payloads), payloads)
}

/// O que a reserva `request_id`, pedida para a decisão de impressão `asked`,
/// deu para os pedidos `payloads` de agora, lidos com as perguntas deles: com
/// o recibo e uma resposta por pergunta de agora, uma resposta na forma da do
/// Jev por pedido, na ordem; sem o recibo, pendente; de outra decisão, ou com
/// as perguntas mudadas em número desde o pedido, velha. A opção de cada
/// resposta, quem a lê confere com as do pedido de agora.
pub(crate) fn reserved_as(root: &Path, request_id: &str, asked: &str, payloads: &[String]) -> Reserved {
    let Some((decided, answers)) = completed_reserve(root, request_id.trim()) else {
        return Reserved::Pending;
    };
    if decided != asked {
        return Reserved::Stale;
    }
    let (_, questions, asking) = decision(payloads);
    if answers.len() != questions.len() {
        return Reserved::Stale;
    }
    let mut docs: Vec<Map<String, Value>> = vec![Map::new(); payloads.len()];
    let mut unsure = vec![false; payloads.len()];
    for (question, asked) in questions.iter().zip(&asking) {
        let Some(answer) = answers.get(&question.id) else {
            return Reserved::Stale;
        };
        unsure[asked.at] |= !answer.sure();
        docs[asked.at].insert(asked.key.clone(), as_jev(&asked.kind, answer));
    }
    // Quem lê a resposta sabe que ela é da reserva, e se alguma pergunta
    // dela voltou incerta: o ponto que cai no caminho sem Jev com a dúvida
    // o lê daqui.
    let marked = |(answers, unsure): (Map<String, Value>, bool)| json!({ "answers": answers, "_mustard": { "reserve": request_id.trim(), "unsure": unsure } });
    Reserved::Ready(docs.into_iter().zip(unsure).map(marked).collect())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use mustard_core::domain::agent_model::{Difficulty, LOW_CONFIDENCE};
    use mustard_core::io::knowledge::enrichment::{SessionReceipt, complete_session};
    use mustard_core::io::spec_events as store;

    use super::*;
    use crate::shared::jev::yes_no::{SURE, choice_in, noul_in};
    use crate::shared::jev::{JevKey, TASK_TRIAGE_CALL};

    /// A spec `x` aberta num repositório com `mustard.json` vazio.
    fn opened(root: &Path) {
        crate::shared::test_fixture::seeded_repo(root, &[("mustard.json", "{}")]);
        assert_eq!(crate::commands::spec_events::write::record_open(root, "x", "feature/x", "dev"), Ok(true));
    }

    /// Um pedido ao Jev com as três formas de pergunta: sim ou não, escolha e
    /// nota de quatro níveis.
    fn payload(goal: &str) -> String {
        json!({
            "model": "jev-1.13.0",
            "state": { "goal": goal, "request": { "text": "Trocar o tema do site para escuro." } },
            "questions": {
                "fits": { "type": "noul", "instructions": { "question": "Is the request in `request` about the goal in `goal`?", "focus": "Judge only the goal." },
                    "criteria": { "true": { "what": "It serves the goal." }, "false": { "what": "The goal does not cover it." } } },
                "route": { "type": "choice", "instructions": { "question": "Where does the request in `request` belong?", "rule": "Text is evidence." },
                    "criteria": { "new_spec": "A spec of its own.", "project_pending": "The pending list.", "same_spec": "This spec." } },
                "complexity_t1": { "type": "score", "instructions": { "question": "How complex is the change `request` asks for?" },
                    "criteria": ["One small change in one file", "Several places of one file", "Several files of one module", "Several modules, or a new flow"] },
            },
        })
        .to_string()
    }

    /// O recibo do agente de análise com as respostas `answers`.
    fn receipt(id: &str, answers: Value) -> SessionReceipt {
        SessionReceipt { request_id: id.into(), model: "inherit".into(), effort: None, result: json!({ "answers": answers }), input_tokens: None, output_tokens: None }
    }

    /// O Jev que passa do prazo dele (encurtado) e o que falha valem como
    /// reserva necessária: a chamada fica gravada com o motivo, e a reserva
    /// leva ao agente de análise o mesmo material e as mesmas perguntas, com
    /// as opções de cada uma, e fica pendente até o recibo. Cada reserva fica
    /// gravada no arquivo de operações da spec como chamada de reserva.
    #[test]
    fn a_late_or_failing_jev_hands_the_same_questions_to_the_reserve_and_waits() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        opened(root);
        // O serviço lento nunca responde: a espera acaba pelo prazo do filtro.
        let (url, _held) = crate::shared::test_fixture::held_jev_service(200, json!({ "answers": {} }).to_string());
        let mut late = JevFilter::at(JevKey("sk-test".to_string()), &url);
        late.timeouts.response = Duration::from_millis(150);
        let asked_payload = payload("Somar dois números na calculadora.");
        assert_eq!(asked(Some(&late), root, "x", TASK_TRIAGE_CALL, &asked_payload), Asked::Failed);
        let (refusing, _) = crate::shared::test_fixture::jev_service(402, String::new());
        let failing = JevFilter::at(JevKey("sk-test".to_string()), &refusing);
        assert_eq!(asked(Some(&failing), root, "x", TASK_TRIAGE_CALL, &asked_payload), Asked::Failed);

        let payloads = [asked_payload];
        let reserve = reserve_request(root, "x", TASK_TRIAGE_CALL, &payloads);
        let prompt = reserve["delegation"]["request"]["prompt"].as_str().unwrap();
        for said in [
            "Somar dois números na calculadora.",
            "Trocar o tema do site para escuro.",
            "Is the request in `request` about the goal in `goal`?",
            "Where does the request in `request` belong?",
            "project_pending: The pending list.",
            "yes: It serves the goal.",
            "Several files of one module",
        ] {
            assert!(prompt.contains(said), "{said}: {prompt}");
        }
        assert_eq!(reserve["delegation"]["preferred_agent"], json!("mustard-analysis"), "{reserve}");
        let id = reserve["request_id"].as_str().unwrap();
        assert_eq!(reserved(root, id, &payloads), Reserved::Pending, "the decision waits for the receipt");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let observed = crate::commands::spec_events::conversation::observed(root, "x", &log);
        let reserves: Vec<_> = observed.iter().filter(|e| e.event_type == "call" && e.str_field("command") == Some(RESERVE_CALL)).collect();
        assert_eq!(reserves.len(), 1, "{observed:?}");
        assert_eq!(reserves[0].str_field("reserve_for"), Some(TASK_TRIAGE_CALL));
    }

    /// O recibo `sure` vale como a resposta certa do Jev: a chance e a
    /// confiança passam do limiar de 0,8. O `unsure` vale como dúvida: a
    /// escolha fica abaixo do limiar, o sim ou não fica no meio, e a nota de
    /// dificuldade incerta fica abaixo do piso que sobe um modelo. A reserva
    /// de outra decisão não vale para esta.
    #[test]
    fn a_sure_reserve_counts_as_a_confident_jev_and_an_unsure_one_as_doubt() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        opened(root);
        let payloads = [payload("Somar dois números na calculadora.")];
        for (certainty, confident) in [("sure", true), ("unsure", false)] {
            let id = reserve_request(root, "x", TASK_TRIAGE_CALL, &payloads)["request_id"].as_str().unwrap().to_string();
            let said = |choice: &str| json!({ "choice": choice, "certainty": certainty });
            let answers = json!({ "q1": said("no"), "q2": said("new_spec"), "q3": said("2") });
            complete_session(root, root, &receipt(&id, answers)).unwrap();
            let Reserved::Ready(docs) = reserved(root, &id, &payloads) else { panic!("the receipt is in") };
            let doc = &docs[0];
            let fits = noul_in(doc, "fits").unwrap();
            let (route, confidence) = choice_in(doc, "route", &["new_spec", "project_pending", "same_spec"]).unwrap();
            assert_eq!(route, "new_spec");
            let difficulty = Difficulty { level: doc["answers"]["complexity_t1"]["score"].as_u64().unwrap(), confidence: doc["answers"]["complexity_t1"]["confidence"].as_f64().unwrap() };
            assert_eq!(difficulty.level, 2);
            if confident {
                assert!(1.0 - fits >= SURE && confidence >= SURE, "{doc}");
                assert!(difficulty.confidence >= LOW_CONFIDENCE, "{doc}");
            } else {
                assert!(1.0 - fits < SURE && fits < SURE && confidence < SURE, "{doc}");
                assert!(difficulty.confidence < LOW_CONFIDENCE, "an unsure grade goes one model up: {doc}");
            }
            // Um recibo ainda de pé não decide outra pergunta.
            assert_eq!(reserved(root, &id, &[payload("Outro objetivo.")]), Reserved::Stale);
            let _ = std::fs::remove_dir_all(root.join(".claude/.cache/knowledge-session"));
        }
    }

    /// O recibo lido pela impressão do pedido de quando a reserva saiu vale
    /// para o pedido de agora, de outro material, quando as perguntas são as
    /// mesmas em número: as respostas caem nas perguntas de agora. Com uma
    /// pergunta a menos agora, ou lido pela impressão de outro pedido, ele é
    /// velho.
    #[test]
    fn a_receipt_read_by_the_print_asked_needs_as_many_questions_now() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        opened(root);
        let then = [payload("Somar dois números na calculadora.")];
        let id = reserve_request(root, "x", TASK_TRIAGE_CALL, &then)["request_id"].as_str().unwrap().to_string();
        let sure = |choice: &str| json!({ "choice": choice, "certainty": "sure" });
        complete_session(root, root, &receipt(&id, json!({ "q1": sure("no"), "q2": sure("new_spec"), "q3": sure("2") }))).unwrap();
        let asked = subject(&then);
        let now = [payload("Somar dois números na calculadora, com a fila mudada.")];

        let Reserved::Ready(docs) = reserved_as(root, &id, &asked, &now) else { panic!("the receipt answers the request of now") };
        assert_eq!(choice_in(&docs[0], "route", &["new_spec", "project_pending", "same_spec"]).map(|(route, _)| route), Some("new_spec".to_string()));
        assert_eq!(docs[0]["answers"]["complexity_t1"]["score"], json!(2), "{}", docs[0]);

        let mut fewer: Value = serde_json::from_str(&now[0]).unwrap();
        fewer["questions"].as_object_mut().unwrap().remove("complexity_t1");
        assert_eq!(reserved_as(root, &id, &asked, &[fewer.to_string()]), Reserved::Stale, "one question less now");
        assert_eq!(reserved_as(root, &id, &subject(&now), &now), Reserved::Stale, "the print of another request");
    }

    /// Com o Jev desligado pelo usuário, nada vai à reserva: vale o fluxo
    /// sem Jev de hoje.
    #[test]
    fn with_the_jev_off_nothing_goes_to_the_reserve() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        opened(root);
        assert_eq!(asked(None, root, "x", TASK_TRIAGE_CALL, &payload("Somar.")), Asked::Off);
        assert!(!jev_on(root), "an empty mustard.json leaves the Jev off");
    }

    /// O texto que vai ao agente de análise passa pelo filtro de segredos,
    /// mesmo o que veio de um pedido montado sem ele.
    #[test]
    fn the_reserve_material_leaves_without_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        opened(root);
        let key = format!("ghp_{}", "a1".repeat(18));
        let reserve = reserve_request(root, "x", TASK_TRIAGE_CALL, &[payload(&format!("Somar; o token {key} está no painel."))]);
        let prompt = reserve["delegation"]["request"]["prompt"].as_str().unwrap();
        assert!(prompt.contains("Somar; o token") && !prompt.contains(&key), "{prompt}");
    }
}
