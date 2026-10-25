//! O resumo do projeto e as regras que cada spec entregue deixa, feitos pelo
//! programa: a linha da spec no resumo, com o texto que ela já tem, e um
//! arquivo de regras do Claude Code para as decisões e regras dela que valem
//! para o projeto inteiro. O Jev só responde sim ou não a cada item; nenhum
//! modelo escreve texto.
//!
//! A entrega grava só a linha, sem regra e sem chamar o Jev, e deixa as
//! regras da spec pendentes ([`record_delivered`]): ela roda na porta da
//! entrega, às vezes com as travas das cópias e do git presas. A decisão das
//! regras vem depois, na passagem das pendências ([`run_pending`]), que roda
//! no `run open` e no `run upsert`, fora de qualquer trava da rodada. A
//! pendência só sai com a resposta do Jev, ou com ele desligado; a falha da
//! chamada a deixa para a passagem seguinte e manda a decisão à reserva do
//! Jev ([`crate::shared::jev::reserve`]), cujo recibo decide no lugar dele.
//!
//! As specs de antes desta versão entram uma vez, na mesma passagem: ganham
//! a linha e ficam com as regras pendentes.
//!
//! Na abertura de uma spec nova, o condutor recebe com o levantamento as
//! entregas ligadas ao objetivo dela ([`for_goal`]); os agentes de onda não.

use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::domain::project_summary::{self as summary, Agreed, Entry, GOAL_CHARS, SURE};
use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::SpecState as _;
use mustard_core::domain::text::cut_line;
use mustard_core::io::project_summary as disk;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use crate::shared::jev::yes_no::{self, YesNo};
use crate::shared::jev::{RULES_CALL, SUMMARY_CALL};
use crate::shared::secret::without_secrets;
use crate::shared::spec_state::DiskSpecState;

/// Quantos caracteres do texto de um item vão ao Jev.
const ITEM_CHARS: usize = 1_000;

/// A pergunta que decide se o item da spec vale para o projeto inteiro.
fn rules_question(goal: &str) -> YesNo<'static> {
    let mut context = Map::new();
    context.insert("spec_goal".to_string(), json!(without_secrets(goal)));
    YesNo {
        context,
        section: "items",
        question: "Does `{entry}` hold for the whole project, beyond the work of this spec?",
        focus: "Answer for future work in this project. An item that only settles a choice inside this spec's work is false.",
        yes: "Any future work in this project must follow the item",
        no: "The item only settles something inside the work of this spec",
    }
}

/// A chave do item `id` na pergunta.
fn item_key(id: u64) -> String {
    format!("i{id}")
}

/// O que o Jev vê de cada item: o tipo, o título, o texto e o porquê ou o
/// exemplo, sem segredo e com teto.
fn item_entries(items: &[Agreed]) -> Vec<(String, Value)> {
    items
        .iter()
        .map(|item| {
            let note = if item.kind == "decision" { "why" } else { "example" };
            let mut shown = json!({
                "kind": item.kind,
                "text": without_secrets(&cut_line(&item.text, ITEM_CHARS)),
            });
            if !item.title.is_empty() {
                shown["title"] = json!(without_secrets(&item.title));
            }
            if !item.note.is_empty() {
                shown[note] = json!(without_secrets(&cut_line(&item.note, ITEM_CHARS)));
            }
            (item_key(item.id), shown)
        })
        .collect()
}

/// A chance de cada item pelo número dele, lida das chances por chave.
fn by_item(items: &[Agreed], chances: Option<&BTreeMap<String, f64>>) -> BTreeMap<u64, f64> {
    let Some(chances) = chances else { return BTreeMap::new() };
    items.iter().filter_map(|item| chances.get(&item_key(item.id)).map(|chance| (item.id, *chance))).collect()
}

/// O custo de cada spec que os recibos do projeto `root` conhecem.
fn costs(root: &Path) -> BTreeMap<String, u64> {
    crate::commands::panel::spec_costs(root)
}

/// Grava as regras da spec `spec` e a linha dela no resumo do projeto `root`,
/// a partir do arquivo de eventos `log`, com a chance de sim de cada item em
/// `chances` e o custo conhecido em `costs`.
fn write_entry(root: &Path, spec: &str, log: &SpecLog, items: &[Agreed], chances: &BTreeMap<u64, f64>, costs: &BTreeMap<String, u64>) -> mustard_core::platform::error::Result<Entry> {
    let lang = mustard_core::ProjectConfig::load(root).language().text_or_default();
    let groups = summary::group_rules(items, chances);
    let rules = disk::write_rules(root, spec, &groups, lang)?;
    let (title, delivered) = summary::title_and_delivered(log, spec, lang);
    let entry = Entry {
        spec: spec.to_string(),
        delivered_at: summary::delivered_at(log),
        pr: summary::pr_of(log),
        title,
        delivered,
        rules,
        skills: crate::shared::failure_skills::skills_of(log),
        cost: costs.get(spec).map(|micro| json!({ "micro_usd": micro, "basis": "host-api-equivalent-estimate" })),
    };
    disk::upsert(root, &entry)?;
    Ok(entry)
}

/// O objetivo da spec, para o estado da pergunta das regras.
fn goal(log: &SpecLog) -> String {
    mustard_core::domain::spec_index::goal_of(log).unwrap_or_default()
}

/// Grava o que a spec `spec`, vista de `start`, deixa ao projeto na entrega:
/// a linha dela no resumo, sem regra — as que ela deixou antes saem —, e a
/// marca de que as regras dela esperam a decisão do Jev. Não chama o Jev.
///
/// # Errors
///
/// A spec que não se lê e a falha de gravação, com o motivo; quem chama avisa
/// e a entrega segue.
pub(crate) fn record_delivered(start: &Path, spec: &str) -> Result<(), String> {
    let root = store::spec_root(start);
    let log = DiskSpecState::new(start).log(spec).ok_or_else(|| format!("spec-unreadable: {spec}"))?;
    let items = summary::agreed_items(&log);
    write_entry(&root, spec, &log, &items, &BTreeMap::new(), &costs(&root)).map_err(|error| error.to_string())?;
    disk::add_pending_rules(&root, spec).map_err(|error| error.to_string())
}

/// A passagem única das specs antigas do projeto de `start`: cada uma
/// entregue, ou fechada com o pull request gravado, ganha a linha no resumo e
/// fica com as regras pendentes. Roda só com a lista das antigas gravada e
/// enquanto a marca da passagem não existe; no fim, deixa a marca. Não chama
/// o Jev. `Ok(true)` quando rodou.
///
/// # Errors
///
/// A falha de gravação, com o motivo; a marca não é deixada, e a próxima
/// montagem ou abertura tenta de novo.
fn legacy_pass(start: &Path) -> Result<bool, String> {
    let root = store::spec_root(start);
    if disk::legacy_pass_done(&root) || !mustard_core::io::fs::exists(mustard_core::io::legacy_specs::path(&root)) {
        return Ok(false);
    }
    let legacy = mustard_core::io::legacy_specs::LegacySpecs::read(&root);
    let known = costs(&root);
    let mut summarized = 0;
    for name in legacy.names() {
        let Some(log) = store::spec_file(&root, name).ok().and_then(|file| store::read(&file).ok().flatten()) else { continue };
        if !summary::summarizable(&log) {
            continue;
        }
        write_entry(&root, name, &log, &summary::agreed_items(&log), &BTreeMap::new(), &known).map_err(|error| error.to_string())?;
        disk::add_pending_rules(&root, name).map_err(|error| error.to_string())?;
        summarized += 1;
    }
    disk::mark_legacy_pass(&root, &mustard_core::harness_version(), summarized).map_err(|error| error.to_string())?;
    Ok(true)
}

/// A passagem das regras pendentes do projeto de `start`, com o Jev do
/// projeto. Devolve quantas specs saíram da espera.
///
/// # Errors
///
/// A falha de gravação, com o motivo.
fn pending_rules_pass(start: &Path) -> Result<(usize, Option<Value>), String> {
    let root = store::spec_root(start);
    let jev = yes_no::project_jev(&root, "", RULES_CALL);
    pending_rules_reserved(&root, jev.as_deref())
}

/// [`pending_rules_with`], com a reserva do Jev: as perguntas que o Jev
/// ligado não responde vão à reserva dele, e o pedido volta junto; as specs
/// ficam na espera até o recibo, que decide no lugar do Jev.
fn pending_rules_reserved(root: &Path, jev: Option<&yes_no::AskFn<'_>>) -> Result<(usize, Option<Value>), String> {
    let reserve = std::cell::RefCell::new(None);
    let decide = |groups: &[(&YesNo<'_>, &[(String, Value)])]| match yes_no::ask_all_reserved(root, "", RULES_CALL, groups, jev) {
        yes_no::Decided::Chances(chances) => chances,
        yes_no::Decided::Off => groups.iter().map(|_| Some(BTreeMap::new())).collect(),
        yes_no::Decided::Reserve(request) => {
            *reserve.borrow_mut() = Some(request);
            groups.iter().map(|_| None).collect()
        }
    };
    let done = pending_rules_by(root, &decide)?;
    Ok((done, reserve.into_inner()))
}

/// [`pending_rules_pass`] no projeto `root`, com o Jev `jev`: `None` quando
/// ele está desligado. As perguntas de todas as specs pendentes vão numa
/// chamada só. A spec cuja resposta veio grava as regras, as põe na linha
/// dela e sai da espera; com o Jev desligado, sai sem regra; a falha da
/// chamada, ou a resposta que não se lê, a deixa na espera. A spec que não se
/// lê mais sai da espera sem nada.
#[cfg(test)]
fn pending_rules_with(root: &Path, jev: Option<&yes_no::AskFn<'_>>) -> Result<usize, String> {
    pending_rules_by(root, &|groups| match jev {
        Some(invoke) => yes_no::ask_all(groups, invoke),
        None => groups.iter().map(|_| Some(BTreeMap::new())).collect(),
    })
}

/// A chance de sim de cada item, por grupo de perguntas, ou `None` no grupo
/// que fica na espera.
type Decide<'a> = dyn Fn(&[(&YesNo<'_>, &[(String, Value)])]) -> Vec<Option<BTreeMap<String, f64>>> + 'a;

/// A passagem das regras pendentes do projeto `root`, com a decisão de cada
/// spec dada por `decide`. A spec que não se lê mais sai da espera sem nada.
fn pending_rules_by(root: &Path, decide: &Decide<'_>) -> Result<usize, String> {
    let pending = disk::pending_rules(root);
    if pending.is_empty() {
        return Ok(0);
    }
    let mut done: Vec<String> = Vec::new();
    let mut specs: Vec<(String, SpecLog)> = Vec::new();
    for name in pending {
        match store::spec_file(root, &name).ok().and_then(|file| store::read(&file).ok().flatten()) {
            Some(log) => specs.push((name, log)),
            None => done.push(name),
        }
    }
    let items: Vec<Vec<Agreed>> = specs.iter().map(|(_, log)| summary::agreed_items(log)).collect();
    let questions: Vec<YesNo<'_>> = specs.iter().map(|(_, log)| rules_question(&goal(log))).collect();
    let entries: Vec<Vec<(String, Value)>> = items.iter().map(|items| item_entries(items)).collect();
    let groups: Vec<(&YesNo<'_>, &[(String, Value)])> = questions.iter().zip(&entries).map(|(question, entries)| (question, entries.as_slice())).collect();
    let answers = if groups.is_empty() { Vec::new() } else { decide(&groups) };
    let known = costs(root);
    for (((spec, log), items), chances) in specs.iter().zip(&items).zip(&answers) {
        let Some(chances) = chances else { continue };
        write_entry(root, spec, log, items, &by_item(items, Some(chances)), &known).map_err(|error| error.to_string())?;
        done.push(spec.clone());
    }
    disk::remove_pending_rules(root, &done).map_err(|error| error.to_string())?;
    Ok(done.len())
}

/// O que as passagens das pendências deixam: os avisos das que falharam e
/// os pedidos de reserva das decisões que esperam o recibo.
#[derive(Debug, Default)]
pub(crate) struct Passed {
    pub(crate) warnings: Vec<String>,
    pub(crate) reserves: Vec<Value>,
}

/// As passagens que a abertura de spec e a montagem fazem no projeto de
/// `start`: a única das specs antigas, a das regras pendentes e a das falhas
/// do projeto, nessa ordem.
/// Devolve os avisos das que falharam, em `lang`, e os pedidos de reserva das
/// decisões que o Jev não deu; a próxima tenta de novo.
pub(crate) fn run_pending(start: &Path, lang: Locale) -> Passed {
    let mut passed = Passed::default();
    if let Err(detail) = legacy_pass(start) {
        passed.warnings.push(translate("spec_index.legacy_summary_failed", lang).replace("{detail}", &detail));
    }
    match pending_rules_pass(start) {
        Ok((_, reserve)) => passed.reserves.extend(reserve),
        Err(detail) => passed.warnings.push(translate("spec_index.rules_pending_failed", lang).replace("{detail}", &detail)),
    }
    // As falhas do projeto que esperam grupo, e a tarefa de criar a skill do
    // grupo que se repetiu e ainda espera uma spec que rode.
    let settled = crate::shared::failure_skills::settle(start, "", lang);
    passed.warnings.extend(settled.warnings);
    passed.reserves.extend(settled.reserves);
    passed
}

/// As entregas do projeto `root` ligadas ao objetivo `goal` da spec `spec`,
/// para o condutor, em `lang`: as que o Jev diz que têm a ver com o objetivo,
/// com chance de [`SURE`] para cima, até [`GOAL_CHARS`]. Com o Jev desligado,
/// as mais recentes que cabem, marcadas como recentes. O Jev ligado que não
/// responde manda a escolha à reserva dele: vai o pedido, em `reserve`, e o
/// recibo escolhe no levantamento seguinte. `None` sem entrega no resumo, ou
/// quando nenhuma tem a ver com o objetivo.
pub(crate) fn for_goal(root: &Path, spec: &str, goal: &str, lang: Locale) -> Option<Value> {
    let root = store::spec_root(root);
    let jev = yes_no::project_jev(&root, spec, SUMMARY_CALL);
    for_goal_with(&root, spec, goal, lang, jev.as_deref())
}

/// [`for_goal`] com o Jev `jev`: `None` quando ele está desligado.
fn for_goal_with(root: &Path, spec: &str, goal: &str, lang: Locale, jev: Option<&yes_no::AskFn<'_>>) -> Option<Value> {
    let entries = disk::read(root);
    if entries.is_empty() {
        return None;
    }
    let mut context = Map::new();
    context.insert("goal".to_string(), json!(without_secrets(goal)));
    let question = YesNo {
        context,
        section: "deliveries",
        question: "Is `{entry}` related to `goal`?",
        focus: "True when the earlier delivery changed or decided something that the new goal touches. Deliveries about other parts of the project are false.",
        yes: "The delivery touched what the new goal is about",
        no: "The delivery is about another part of the project",
    };
    let key = |at: usize| format!("d{}", at + 1);
    let shown: Vec<(String, Value)> = entries
        .iter()
        .enumerate()
        .map(|(at, entry)| (key(at), json!({ "title": without_secrets(&entry.title), "delivered": without_secrets(&entry.delivered) })))
        .collect();
    let answered = match yes_no::ask_all_reserved(root, spec, SUMMARY_CALL, &[(&question, &shown)], jev) {
        yes_no::Decided::Chances(chances) => chances.into_iter().next().flatten(),
        yes_no::Decided::Off => None,
        yes_no::Decided::Reserve(request) => {
            return Some(json!({ "chosen": "reserve", "hint": translate("survey.project_summary.reserve", lang), "reserve": request }));
        }
    };
    let (chosen, hint) = match answered {
        Some(chances) => {
            let related = entries.iter().enumerate().filter(|(at, _)| chances.get(&key(*at)).is_some_and(|chance| *chance >= SURE)).map(|(_, entry)| entry);
            ("jev", summary::within(related, GOAL_CHARS))
        }
        None => ("recent", summary::within(&entries, GOAL_CHARS)),
    };
    if hint.is_empty() {
        return None;
    }
    let text = translate(if chosen == "jev" { "survey.project_summary" } else { "survey.project_summary.recent" }, lang);
    Some(json!({ "chosen": chosen, "hint": text, "entries": hint }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::record_phase;

    /// A resposta a cada pedido, com a chance de cada chave dada por `chance`.
    fn answering(chance: impl Fn(&str, &Value) -> f64) -> impl Fn(&[String]) -> Option<Vec<Value>> {
        move |payloads| {
            Some(
                payloads
                    .iter()
                    .map(|payload| {
                        let body: Value = serde_json::from_str(payload).unwrap();
                        let answers: Map<String, Value> = body["questions"]
                            .as_object()
                            .unwrap()
                            .keys()
                            .map(|key| (key.clone(), json!({"type":"noul","noul":chance(key, &body)})))
                            .collect();
                        json!({ "answers": answers })
                    })
                    .collect(),
            )
        }
    }

    /// A spec `spec` de `root` com os eventos `lines`, numerados de 1 em
    /// diante: o objetivo, uma decisão do projeto (3), uma da obra (4), a
    /// tarefa que cobre a primeira, o resumo do pull request e a fase `phase`.
    fn spec_in(root: &Path, spec: &str, phase: &str) {
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let lines = [
            json!({"type":"state","phase":"approved","author":"binary"}),
            json!({"type":"context","title":format!("Entregar a obra {spec}"),"text":format!("Entregar a obra {spec} com a saída em JSON.")}),
            json!({"type":"decision","title":"Resposta em JSON","text":"Toda resposta do binário sai em JSON.","why":"Quem lê é programa.","keys":["json"]}),
            json!({"type":"decision","title":"Nome da tela","text":"A tela nova se chama Painel.","why":"Pedido do usuário.","keys":["tela"]}),
            json!({"type":"task","title":"Saída","text":"Muda a saída.","agent":"src/out.rs","covers":[3],"files":[{"path":"src/out.rs"}],"wave":1}),
            json!({"type":"pr_summary","text":"Grava a saída em JSON."}),
            json!({"type":"state","phase":phase,"author":"binary"}),
        ];
        let folder = root.join(".claude/spec").join(spec);
        std::fs::create_dir_all(&folder).unwrap();
        let body: String = lines
            .iter()
            .enumerate()
            .filter(|(at, line)| *at + 1 < lines.len() || line["phase"] != "approved")
            .map(|(at, line)| {
                let mut line = line.clone();
                line["v"] = json!(1);
                line["id"] = json!(at + 1);
                line["at"] = json!(format!("2026-10-1{at}T10:00:00Z"));
                line.to_string() + "\n"
            })
            .collect();
        std::fs::write(folder.join("spec.ndjson"), body).unwrap();
    }

    /// A passagem das pendências grava o arquivo de regras com a decisão que
    /// o Jev deu como do projeto, nos caminhos da tarefa que a cobre, e o põe
    /// na linha; a que fica em dúvida fica só na spec. Entregue de novo, a
    /// linha não repete, as regras saem e voltam à espera.
    #[test]
    fn the_pending_pass_records_the_project_rules_in_the_summary_line() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "saida", "delivered");
        record_delivered(root, "saida").unwrap();
        let invoke = answering(|key, _| if key == "i3" { 0.92 } else { 0.6 });
        assert_eq!(pending_rules_with(root, Some(&invoke)), Ok(1));

        let entries = disk::read(root);
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0].spec, "saida");
        assert_eq!(entries[0].title, "Entregar a obra saida");
        assert_eq!(entries[0].delivered, "Grava a saída em JSON.");
        assert_eq!(entries[0].delivered_at, "2026-10-16T10:00:00Z");
        assert_eq!(entries[0].skills, Vec::<String>::new());
        assert_eq!(entries[0].rules.len(), 1, "{:?}", entries[0].rules);
        assert_eq!(entries[0].rules[0].file, ".claude/rules/mustard/saida-1.md");
        assert_eq!(entries[0].rules[0].paths, vec!["src/out.rs"]);
        let text = std::fs::read_to_string(root.join(".claude/rules/mustard/saida-1.md")).unwrap();
        assert!(text.starts_with("---\npaths:\n  - \"src/out.rs\"\n---\n"), "{text}");
        assert!(text.contains("- Toda resposta do binário sai em JSON. — Quem lê é programa."), "{text}");
        assert!(!text.contains("Painel"), "the item in doubt stays only in the spec: {text}");

        record_delivered(root, "saida").unwrap();
        let again = disk::read(root);
        assert_eq!(again.len(), 1, "the same spec is rewritten, not repeated");
        assert!(again[0].rules.is_empty());
        assert!(!root.join(".claude/rules/mustard/saida-1.md").exists());
        assert_eq!(disk::pending_rules(root), vec!["saida"]);
    }

    /// Um Jev de teste que conta as chamadas e responde com `chance`.
    fn counting(calls: &std::rc::Rc<std::cell::Cell<usize>>, chance: f64) -> yes_no::FakeAsk {
        let calls = std::rc::Rc::clone(calls);
        let answer = answering(move |key, _| if key == "i3" { chance } else { 0.6 });
        std::rc::Rc::new(move |payloads: &[String]| {
            calls.set(calls.get() + 1);
            answer(payloads)
        })
    }

    /// A entrega não chama o Jev: grava a linha sem regras e deixa as regras
    /// da spec pendentes. A passagem das pendências, a da abertura seguinte,
    /// chama o Jev uma vez, grava as regras, as põe na linha e consome a
    /// pendência.
    #[test]
    fn the_delivery_asks_nothing_and_leaves_the_rules_pending() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        yes_no::ask_in_test(Some(counting(&calls, 0.92)));
        spec_in(root, "porta", "approved");
        for phase in ["running", "closed", "delivered"] {
            assert!(record_phase(root, "porta", phase, None), "{phase}");
        }
        assert_eq!(calls.get(), 0, "the delivery never calls the Jev");
        assert_eq!(disk::pending_rules(root), vec!["porta"]);
        assert!(disk::read(root)[0].rules.is_empty());

        assert!(run_pending(root, Locale::PtBr).warnings.is_empty());
        yes_no::ask_in_test(None);
        assert_eq!(calls.get(), 1);
        assert!(disk::pending_rules(root).is_empty(), "the answer consumes the pending rules");
        let rules = &disk::read(root)[0].rules;
        assert_eq!(rules.iter().map(|file| file.file.as_str()).collect::<Vec<_>>(), vec![".claude/rules/mustard/porta-1.md"]);
        assert_eq!(rules[0].paths, vec!["src/out.rs"]);
    }

    /// A falha do Jev não consome a passagem: as regras das antigas ficam
    /// pendentes e a passagem seguinte, com resposta, as decide. Com o Jev
    /// desligado não há o que esperar: a pendência sai sem regra.
    #[test]
    fn a_failed_jev_keeps_the_rules_pending_and_an_answer_consumes_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "velha", "delivered");
        mustard_core::io::legacy_specs::mark_once(root, "0.2.25").unwrap();
        yes_no::ask_in_test(Some(std::rc::Rc::new(|_: &[String]| None)));
        run_pending(root, Locale::PtBr);
        assert_eq!(disk::read(root).iter().map(|e| e.spec.as_str()).collect::<Vec<_>>(), vec!["velha"], "the line is written anyway");
        assert_eq!(disk::pending_rules(root), vec!["velha"], "a failure keeps the rules pending");
        assert!(!root.join(".claude/rules/mustard/velha-1.md").exists());

        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        yes_no::ask_in_test(Some(counting(&calls, 0.95)));
        run_pending(root, Locale::PtBr);
        assert_eq!(calls.get(), 1);
        assert!(disk::pending_rules(root).is_empty(), "the answer consumes it");
        assert_eq!(disk::read(root)[0].rules.len(), 1);

        spec_in(root, "desligada", "approved");
        assert!(record_phase(root, "desligada", "delivered", None));
        yes_no::ask_in_test(None);
        run_pending(root, Locale::PtBr);
        assert!(disk::pending_rules(root).is_empty(), "with the Jev off there is nothing to wait for");
    }

    /// O Jev ligado que falha manda as regras pendentes à reserva: a
    /// passagem traz o pedido com a pergunta de sim ou não de cada item, e a
    /// pendência fica até o recibo; a passagem seguinte, ainda sem ele, traz
    /// o mesmo pedido. O recibo certo grava as regras como o Jev e consome a
    /// pendência, sem chamar o Jev; o incerto vale como dúvida: sai sem regra.
    /// Com o Jev desligado, nada vai à reserva.
    #[test]
    fn a_failed_jev_sends_the_pending_rules_to_the_reserve_and_the_receipt_applies() {
        for certainty in ["sure", "unsure"] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            spec_in(root, "saida", "delivered");
            record_delivered(root, "saida").unwrap();
            yes_no::ask_in_test(Some(std::rc::Rc::new(|_: &[String]| None)));
            let passed = run_pending(root, Locale::PtBr);
            assert_eq!(passed.reserves.len(), 1, "{passed:?}");
            let request = passed.reserves[0].clone();
            let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap();
            assert!(prompt.contains("Does `items.i3` hold for the whole project") && prompt.contains("Toda resposta do binário"), "{prompt}");
            assert_eq!(disk::pending_rules(root), vec!["saida"], "pending until the receipt");
            let again = run_pending(root, Locale::PtBr);
            assert_eq!(again.reserves.first().map(|r| r["request_id"].clone()), Some(request["request_id"].clone()));

            crate::shared::jev::reserve::answer_in_test(root, &request, |text| (if text.contains("items.i3") { "yes" } else { "no" }).into(), certainty);
            yes_no::ask_in_test(Some(std::rc::Rc::new(|_: &[String]| panic!("the receipt answers in place of the Jev"))));
            let applied = run_pending(root, Locale::PtBr);
            yes_no::ask_in_test(None);
            assert!(applied.reserves.is_empty(), "{applied:?}");
            assert!(disk::pending_rules(root).is_empty(), "the receipt consumes the pending rules");
            let rules = &disk::read(root)[0].rules;
            assert_eq!(rules.len(), usize::from(certainty == "sure"), "{certainty}: {rules:?}");

            spec_in(root, "desligada", "approved");
            assert!(record_phase(root, "desligada", "delivered", None));
            assert!(run_pending(root, Locale::PtBr).reserves.is_empty(), "with the Jev off nothing goes to the reserve");
        }
    }

    /// A porta da entrega grava o resumo sozinha, sem o Jev nos testes: a
    /// spec entregue ganha a linha; a fechada, não.
    #[test]
    fn recording_the_delivered_phase_writes_the_summary_line() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "porta", "approved");
        assert!(record_phase(root, "porta", "running", None));
        assert!(record_phase(root, "porta", "closed", None));
        assert!(disk::read(root).is_empty(), "closing writes no line");
        assert!(record_phase(root, "porta", "delivered", None));
        let entries = disk::read(root);
        assert_eq!(entries.iter().map(|e| e.spec.as_str()).collect::<Vec<_>>(), vec!["porta"], "{entries:?}");
    }

    /// A passagem única dá linha às antigas entregues sem chamar o Jev, deixa
    /// as regras delas pendentes e a marca, e não roda de novo; sem a lista
    /// das antigas, não roda. As pendências de todas vão numa chamada só.
    #[test]
    fn the_legacy_pass_runs_once_and_the_rules_of_every_old_spec_go_in_one_call() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "velha", "delivered");
        spec_in(root, "outra", "delivered");
        spec_in(root, "em-obra", "running");
        assert_eq!(legacy_pass(root), Ok(false), "no list, no pass");

        mustard_core::io::legacy_specs::mark_once(root, "0.2.25").unwrap();
        assert_eq!(legacy_pass(root), Ok(true));
        let mut specs: Vec<String> = disk::read(root).into_iter().map(|e| e.spec).collect();
        specs.sort();
        assert_eq!(specs, vec!["outra", "velha"], "the spec still in work gets no line");
        assert_eq!(disk::pending_rules(root), vec!["outra", "velha"]);
        assert_eq!(legacy_pass(root), Ok(false), "the pass runs once");

        let calls = std::cell::Cell::new(0);
        let answer = answering(|_, _| 0.95);
        let invoke = |payloads: &[String]| {
            calls.set(calls.get() + 1);
            answer(payloads)
        };
        assert_eq!(pending_rules_with(root, Some(&invoke)), Ok(2));
        assert_eq!(calls.get(), 1, "one call for every pending spec");
        assert!(root.join(".claude/rules/mustard/velha-1.md").exists());
        assert!(root.join(".claude/rules/mustard/velha-2.md").exists(), "the item without paths goes to its own file");
    }

    /// O Jev ligado que falha na escolha das entregas ligadas ao objetivo
    /// manda a escolha à reserva: o levantamento traz o pedido, com a mesma
    /// pergunta de cada entrega, em vez das recentes. Com o recibo certo, as
    /// entregas que ele liga ao objetivo, sem chamar o Jev; com o incerto,
    /// dúvida: nenhuma. Com o Jev desligado, as recentes, sem reserva.
    #[test]
    fn a_failed_jev_sends_the_goal_deliveries_to_the_reserve_and_the_receipt_chooses() {
        for certainty in ["sure", "unsure"] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            std::fs::write(root.join("mustard.json"), "{}").unwrap();
            let entry = |spec: &str, at: &str, title: &str| Entry {
                spec: spec.into(),
                delivered_at: at.into(),
                pr: None,
                title: title.into(),
                delivered: "Saiu.".into(),
                rules: vec![],
                skills: vec![],
                cost: None,
            };
            disk::upsert(root, &entry("saida", "2026-10-01T10:00:00Z", "Saída em JSON")).unwrap();
            disk::upsert(root, &entry("painel", "2026-10-05T10:00:00Z", "Painel novo")).unwrap();
            let key = format!("deliveries.d{}", disk::read(root).iter().position(|e| e.spec == "saida").unwrap() + 1);

            yes_no::ask_in_test(Some(std::rc::Rc::new(|_: &[String]| None)));
            let out = for_goal(root, "", "Mexer na saída.", Locale::PtBr).unwrap();
            assert_eq!(out["chosen"], "reserve", "{out}");
            assert!(out["hint"].as_str().unwrap().contains("`reserve`"), "{out}");
            let prompt = out["reserve"]["delegation"]["request"]["prompt"].as_str().unwrap();
            assert!(prompt.contains(&format!("Is `{key}` related to `goal`?")) && prompt.contains("Mexer na saída."), "{prompt}");

            let request = out["reserve"].clone();
            crate::shared::jev::reserve::answer_in_test(root, &request, |text| (if text.contains(&key) { "yes" } else { "no" }).into(), certainty);
            yes_no::ask_in_test(Some(std::rc::Rc::new(|_: &[String]| panic!("the receipt answers in place of the Jev"))));
            let chosen = for_goal(root, "", "Mexer na saída.", Locale::PtBr);
            yes_no::ask_in_test(None);
            if certainty == "sure" {
                let chosen = chosen.unwrap();
                assert_eq!(chosen["chosen"], "jev", "{chosen}");
                assert_eq!(chosen["entries"].as_array().unwrap().iter().map(|e| e["spec"].as_str().unwrap()).collect::<Vec<_>>(), vec!["saida"]);
            } else {
                assert_eq!(chosen, None, "an unsure receipt is a doubt");
            }
            let off = for_goal(root, "", "Outro objetivo.", Locale::PtBr).unwrap();
            assert_eq!(off["chosen"], "recent", "with the Jev off: {off}");
        }
    }

    /// Com o objetivo, só as entregas que o Jev liga a ele, com certeza; sem
    /// o Jev, as mais recentes, marcadas; sem entrega, nada.
    #[test]
    fn the_goal_gets_the_related_deliveries_or_the_most_recent_ones() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(for_goal_with(root, "", "Mexer na saída.", Locale::PtBr, Some(&|_: &[String]| panic!("nothing to ask"))), None);
        let entry = |spec: &str, at: &str, title: &str| Entry {
            spec: spec.into(),
            delivered_at: at.into(),
            pr: None,
            title: title.into(),
            delivered: "Saiu.".into(),
            rules: vec![],
            skills: vec![],
            cost: None,
        };
        disk::upsert(root, &entry("saida", "2026-10-01T10:00:00Z", "Saída em JSON")).unwrap();
        disk::upsert(root, &entry("painel", "2026-10-05T10:00:00Z", "Painel novo")).unwrap();

        let related = answering(|key, body| if body["state"]["deliveries"][key]["title"] == "Saída em JSON" { 0.9 } else { 0.79 });
        let chosen = for_goal_with(root, "", "Mexer na saída.", Locale::PtBr, Some(&related)).unwrap();
        assert_eq!(chosen["chosen"], "jev");
        assert_eq!(chosen["entries"].as_array().unwrap().iter().map(|e| e["spec"].as_str().unwrap()).collect::<Vec<_>>(), vec!["saida"]);

        let recent = for_goal_with(root, "", "Mexer na saída.", Locale::EnUs, None).unwrap();
        assert_eq!(recent["chosen"], "recent");
        assert_eq!(recent["entries"].as_array().unwrap().iter().map(|e| e["spec"].as_str().unwrap()).collect::<Vec<_>>(), vec!["painel", "saida"]);
        assert!(recent["hint"].as_str().unwrap().starts_with("The project's most recent deliveries"));

        assert_eq!(for_goal_with(root, "", "Outra coisa.", Locale::PtBr, Some(&answering(|_, _| 0.2))), None, "nothing related, nothing shown");
    }
}
