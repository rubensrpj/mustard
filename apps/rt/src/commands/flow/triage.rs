//! `mustard-rt run triage --spec <spec>` — reorganiza a lista aberta da spec
//! pelo classificador das atividades ([`task_triage::triage_backlog_with`]),
//! numa chamada só ao Jev: os grupos de mesmo sentido, as tarefas que uma
//! entrega já cobre e a ordem dos assuntos por valor.
//!
//! Sem `--apply`, nada é gravado, e a resposta é curta: os grupos com mais de
//! uma tarefa (a primeira fica), as saídas com o motivo e a ordem. Com
//! `--apply`, sob a trava do arquivo de eventos e tudo ou nada, a primeira
//! tarefa de cada grupo ganha a versão nova com o trabalho das outras numa
//! linha da parte do agente, as outras saem com o motivo, e a coberta por uma
//! entrega também sai; a resposta traz, para cada versão nova, o pedido ao
//! agente de análise da sessão que escreve o texto dela
//! ([`task_triage::rewrite_request`]). Vai à classificação toda tarefa ainda
//! por entregar, com onda ou sem: a de onda que voltou sem pedir mudança de
//! plano conta como entregue, e pode cobrir outra. A gravação mexe também na
//! tarefa de onda parada ou sem agente, que sai da onda na versão nova, e a
//! onda que fica sem tarefa sai do ar; só a tarefa de onda com agente vivo
//! fica de fora da gravação, com aviso. A resposta diz quantas foram
//! classificadas e quantas ficaram de fora da gravação. Sem leitura do Jev, a
//! lista fica como está, com aviso.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use crate::commands::flow::round::queue::{task_files, tasks_not_delivered, waves_returned_without_replan};
use crate::commands::flow::round::{append_agent_line, backlog_return, waves_in_progress, waves_left_without_tasks};
use crate::commands::spec_events::{self, write::record_locked};
use crate::shared::spec_state::{DiskSpecState, checkout, session_from_env};
use crate::shared::task_triage::{self, BacklogTriage, Candidate};

/// Options for `mustard-rt run triage`.
pub struct TriageOpts {
    pub root: PathBuf,
    pub spec: Option<String>,
    /// Grava a reorganização em vez de só mostrá-la.
    pub apply: bool,
}

/// O relatório do `run triage`, com a pergunta ao Jev da orquestração.
pub(crate) fn triage_at(opts: &TriageOpts) -> Value {
    triage_with(opts, session_from_env().as_deref(), &|root, spec, payload| task_triage::jev_ask(root, spec)(payload))
}

/// [`triage_at`] com a sessão recebida e a pergunta feita por `ask`, que
/// recebe a raiz do projeto, a spec e o pedido.
fn triage_with(opts: &TriageOpts, session: Option<&str>, ask: &dyn Fn(&Path, &str, &str) -> Option<Value>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    let refuse = |refusal: &Refusal| spec_events::refused(refusal, lang);
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => match DiskSpecState::new(&checkout(&opts.root)).active(session) {
            Some(spec) => spec,
            None => return refuse(&Refusal::NoCurrentSpec),
        },
    };
    let path = match store::spec_file(&project.root, &spec) {
        Ok(path) => path,
        Err(refusal) => return refuse(&refusal),
    };
    let log = match store::read(&path) {
        Ok(Some(log)) => log,
        Ok(None) => return refuse(&Refusal::SpecNotOpen { spec }),
        Err(refusal) => return refuse(&refusal),
    };
    let unfinished = tasks_not_delivered(&log, &BTreeSet::new());
    // A onda que voltou sem pedir mudança de plano já entregou: as tarefas
    // dela entram como entregues. A de agente vivo entra na classificação e
    // fica de fora da gravação.
    let delivered = waves_returned_without_replan(&log);
    let live: BTreeSet<u64> = waves_in_progress(&log).into_keys().collect();
    let wave_of = |id: &u64| log.get(*id).and_then(SpecEvent::wave);
    let classified: BTreeSet<u64> = unfinished.iter().copied().filter(|id| wave_of(id).is_none_or(|w| !delivered.contains(&w))).collect();
    let held: BTreeSet<u64> = classified.iter().copied().filter(|id| wave_of(id).is_some_and(|w| live.contains(&w))).collect();
    let codes = log.codes();
    let code = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
    fn candidate(task: &SpecEvent) -> Candidate<'_> {
        Candidate {
            id: task.id,
            title: task.str_field("title").unwrap_or_default(),
            text: task.str_field("text").unwrap_or_default(),
            files: task_files(task),
            joinable: true,
        }
    }
    let open: Vec<Candidate> = classified.iter().filter_map(|id| log.get(*id)).map(candidate).collect();
    let done: Vec<Candidate> =
        log.visible().into_iter().filter(|e| e.event_type == "task" && !classified.contains(&e.id)).map(candidate).collect();
    let mut warnings: Vec<String> = Vec::new();
    if !held.is_empty() {
        let tasks: Vec<String> = held.iter().map(|id| code(*id)).collect();
        warnings.push(translate("triage.held", lang).replace("{tasks}", &tasks.join(", ")));
    }
    let Some(triage) = task_triage::triage_backlog_with(&open, &done, &|payload| ask(&project.root, &spec, payload)) else {
        warnings.push(translate("triage.no_jev", lang).to_string());
        let order: Vec<String> = open.iter().map(|task| code(task.id)).collect();
        return json!({ "ok": true, "spec": spec, "applied": false, "classified": open.len(), "kept_out": held.len(), "groups": [],
            "covered": [], "order": order, "warnings": warnings });
    };
    let groups: Vec<Value> =
        triage.groups.iter().filter(|group| group.len() > 1).map(|group| json!({ "keep": code(group[0]), "join": group[1..].iter().map(|id| code(*id)).collect::<Vec<_>>() })).collect();
    let covered: Vec<Value> = triage
        .covered
        .iter()
        .map(|(task, by)| json!({ "task": code(*task), "reason": translate("triage.removed_covered", lang).replace("{code}", &code(*by)) }))
        .collect();
    let order: Vec<String> = triage.groups.iter().map(|group| code(group[0])).collect();
    let mut report = json!({ "ok": true, "spec": spec, "applied": false, "classified": open.len(), "kept_out": held.len(), "groups": groups,
        "covered": covered, "order": order });
    if !warnings.is_empty() {
        report["warnings"] = json!(warnings);
    }
    if !opts.apply {
        report["next"] = json!(translate("triage.next_dry", lang));
        return report;
    }
    match apply(&opts.root, &spec, &path, &triage, &held, lang) {
        Ok(Applied { versions, emptied }) => {
            let after = store::read(&path).ok().flatten().unwrap_or(log);
            let rewrite: Vec<Value> = versions.iter().map(|(version, group)| rewrite_of(&project.root, &after, *version, group)).collect();
            report["applied"] = json!(true);
            report["rewrite"] = json!(rewrite);
            if !emptied.is_empty() {
                report["waves_left_without_tasks"] = json!(emptied);
            }
            report["next"] = json!(translate("triage.next_applied", lang));
            report
        }
        Err(refusal) => refuse(&refusal),
    }
}

/// O que [`apply`] gravou: o número de cada versão nova, com as tarefas do
/// grupo que entraram nela, a primeira à frente, e as ondas que ficaram sem
/// tarefa pela gravação e saíram do ar.
struct Applied {
    versions: Vec<(u64, Vec<u64>)>,
    emptied: Vec<u64>,
}

/// Grava a reorganização `triage` na spec `spec`, vista de `start`, sob a
/// trava do arquivo de eventos `path`, sem tocar nas tarefas de `held`, de
/// onda com agente vivo: para cada grupo, sem elas, a versão nova da primeira
/// tarefa ([`joined_version`]), sem onda, e a remoção das outras, com o motivo
/// no idioma `lang`; depois, a remoção de cada tarefa que uma entrega cobre.
/// O grupo que fica com uma tarefa só não grava nada. Uma recusa no meio
/// volta o arquivo ao que era antes da primeira gravação, e nada fica.
fn apply(start: &Path, spec: &str, path: &Path, triage: &BacklogTriage, held: &BTreeSet<u64>, lang: Locale) -> Result<Applied, Refusal> {
    let locked_result = store::with_locked_writer(path, |locked| {
        let before = locked.content().to_string();
        let in_air = waves_left_without_tasks(locked.log());
        let mut record_all = || -> Result<Applied, Refusal> {
            let mut versions = Vec::new();
            for group in &triage.groups {
                let group: Vec<u64> = group.iter().copied().filter(|id| !held.contains(id)).collect();
                if group.len() < 2 {
                    continue;
                }
                let draft = joined_version(locked.log(), &group, lang)?;
                let version = record_locked(locked, start, spec, "task", draft, PhaseWriter::Binary)?.written.id;
                let keeper = locked.log().codes().get(&group[0]).cloned().unwrap_or_else(|| group[0].to_string());
                let removal = json!({ "targets": group[1..], "reason": translate("triage.removed_joined", lang).replace("{code}", &keeper) });
                record_locked(locked, start, spec, "remove", object(removal), PhaseWriter::Binary)?;
                versions.push((version, group));
            }
            for (task, by) in triage.covered.iter().filter(|(task, _)| !held.contains(task)) {
                let delivery = locked.log().codes().get(by).cloned().unwrap_or_else(|| by.to_string());
                let removal = json!({ "targets": [task], "reason": translate("triage.removed_covered", lang).replace("{code}", &delivery) });
                record_locked(locked, start, spec, "remove", object(removal), PhaseWriter::Binary)?;
            }
            let emptied = waves_left_without_tasks(locked.log()).into_keys().filter(|wave| !in_air.contains_key(wave)).collect();
            Ok(Applied { versions, emptied })
        };
        let recorded = record_all();
        if recorded.is_err() {
            locked.restore(before)?;
        }
        recorded
    })?;
    locked_result.unwrap_or_else(|| Err(Refusal::SpecNotOpen { spec: spec.to_string() }))
}

/// A versão nova da primeira tarefa de `group`, lida em `log`, com o trabalho
/// das outras: cada uma numa linha da parte do agente, no idioma `lang`, e os
/// arquivos, os itens cobertos e as dependências dela somados aos da
/// primeira, sem a dependência de uma tarefa do próprio grupo.
fn joined_version(log: &SpecLog, group: &[u64], lang: Locale) -> Result<Map<String, Value>, Refusal> {
    let codes = log.codes();
    let task = |id: u64| log.get(id).ok_or(Refusal::UnknownTarget { target: mustard_core::domain::spec_events::EventRef::Id(id) });
    let mut draft = backlog_return(task(group[0])?);
    let members: Vec<Value> = group.iter().flat_map(|id| [json!(id), json!(codes.get(id).cloned().unwrap_or_default())]).collect();
    for &other in &group[1..] {
        let other = task(other)?;
        let line = translate("triage.joined_line", lang)
            .replace("{code}", &codes.get(&other.id).cloned().unwrap_or_else(|| other.id.to_string()))
            .replace("{title}", other.str_field("title").unwrap_or_default())
            .replace("{text}", &other.str_field("text").unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" "));
        append_agent_line(&mut draft, &line);
        for field in ["files", "covers", "depends_on"] {
            let mut list: Vec<Value> = draft.get(field).and_then(Value::as_array).cloned().unwrap_or_default();
            for value in other.fields.get(field).and_then(Value::as_array).into_iter().flatten() {
                if !list.contains(value) && !(field == "depends_on" && members.contains(value)) {
                    list.push(value.clone());
                }
            }
            draft.insert(field.into(), json!(list));
        }
    }
    Ok(draft)
}

/// O pedido de reescrita da versão nova `version`, lida em `after`, com as
/// tarefas que o grupo `group` juntou nela como as outras fontes.
fn rewrite_of(root: &Path, after: &SpecLog, version: u64, group: &[u64]) -> Value {
    let codes = after.codes();
    let source = |id: u64| after.get(id).map(|event| task_triage::rewrite_source(codes.get(&id).cloned().unwrap_or_else(|| id.to_string()), &event.fields));
    let sources: Vec<_> = std::iter::once(version).chain(group[1..].iter().copied()).filter_map(source).collect();
    json!({ "task": codes.get(&version).cloned().unwrap_or_else(|| version.to_string()), "request": task_triage::rewrite_request(root, version, sources) })
}

/// O objeto JSON de `value`, que é sempre um objeto aqui.
fn object(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{record, record_open};

    fn sure(choice: &str) -> Value {
        json!({ "choice": choice, "confidence": 0.95, "probabilities": { choice: 0.93, "runner-up": 0.04 } })
    }

    /// Uma spec aberta com as tarefas `tasks` no backlog, na ordem: o
    /// título, o arquivo e as posições das anteriores de que cada uma
    /// depende. Devolve os números.
    fn backlog(dir: &Path, tasks: &[(&str, &str, &[usize])]) -> Vec<u64> {
        crate::shared::test_fixture::seeded_repo(dir, &[("mustard.json", "{}")]);
        assert_eq!(record_open(dir, "x", "feature/x", "dev"), Ok(true));
        let said = json!({ "author": "user", "text": "reorganize a lista" });
        let said = record(dir, "x", "message", object(said), PhaseWriter::Binary).unwrap().written.id;
        let mut ids: Vec<u64> = Vec::new();
        for (title, file, depends_on) in tasks {
            let depends_on: Vec<u64> = depends_on.iter().map(|at| ids[*at]).collect();
            let draft = json!({ "title": title, "text": format!("{title}."), "agent": "- conferir pelo teste", "files": [{ "path": file }],
                "depends_on": depends_on, "origin": said });
            ids.push(record(dir, "x", "task", object(draft), PhaseWriter::Binary).unwrap().written.id);
        }
        ids
    }

    fn log_of(dir: &Path) -> SpecLog {
        store::read(&store::spec_file(dir, "x").unwrap()).unwrap().unwrap()
    }

    /// O Jev confiante em que cada tarefa de `same` tem o mesmo sentido da
    /// anterior apontada.
    fn grouped(same: &[(u64, u64)]) -> Value {
        let answers: Map<String, Value> = same.iter().map(|(task, earlier)| (format!("t{task}_same"), sure(&format!("t{earlier}")))).collect();
        json!({ "answers": answers })
    }

    /// Quatro tarefas reais da lista de 16/10: a retomada que lista as ondas
    /// no ar, a busca que não espera a trava do mapa, a retomada que diz a
    /// onda que perdeu o processo e o título do commit. O Jev diz que a
    /// terceira tem o sentido da primeira. Sem `--apply`, a resposta dá o
    /// grupo e nada se grava; com `--apply`, a primeira ganha a versão nova
    /// com a terceira na parte do agente e o arquivo dela, a terceira sai, as
    /// outras ficam, e a resposta traz o pedido de reescrita.
    #[test]
    fn backlog_triage_shows_the_group_and_applies_it_only_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let ids = backlog(root, &[
            ("A retomada lista as ondas no ar", "apps/rt/src/commands/flow/resume.rs", &[]),
            ("A busca não espera a trava do mapa", "apps/rt/src/commands/search.rs", &[]),
            ("A retomada diz a onda que perdeu o processo", "apps/rt/src/commands/flow/stuck.rs", &[]),
            ("O commit leva título e corpo", "apps/rt/src/commands/flow/round/commit.rs", &[]),
        ]);
        let opts = |apply| TriageOpts { root: root.to_path_buf(), spec: Some("x".into()), apply };
        let answer = grouped(&[(ids[2], ids[0])]);
        let events = log_of(root).events.len();

        let shown = triage_with(&opts(false), None, &|_, _, _| Some(answer.clone()));
        assert_eq!(shown["groups"], json!([{ "keep": "MSTD-TASK-0001", "join": ["MSTD-TASK-0003"] }]), "{shown}");
        assert_eq!(shown["order"].as_array().map(Vec::len), Some(3), "{shown}");
        assert_eq!(log_of(root).events.len(), events, "nothing is recorded without --apply");

        let applied = triage_with(&opts(true), None, &|_, _, _| Some(answer.clone()));
        assert_eq!(applied["applied"], json!(true), "{applied}");
        let log = log_of(root);
        let visible: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "task").collect();
        assert_eq!(visible.len(), 3, "{visible:?}");
        let kept = visible.iter().find(|e| e.int("replaces") == Some(ids[0])).expect("the new version of the first");
        assert!(kept.str_field("agent").unwrap().contains("MSTD-TASK-0003"), "{kept:?}");
        assert!(task_files(kept).contains("apps/rt/src/commands/flow/stuck.rs"));
        assert!(visible.iter().all(|e| e.id != ids[2]), "the joined task leaves");
        assert!(applied["rewrite"][0]["request"]["delegation"]["model_family"].is_string(), "{applied}");
    }

    /// Uma tarefa solta e quatro de onda: a de onda parada por mudança de
    /// plano, à espera da resposta; a de onda sem agente, cujo envio é de um
    /// processo que já fechou; a de onda com agente vivo, o processo deste
    /// teste; e a de onda que voltou sem mudança de plano, à espera só da
    /// rodada. O Jev diz que as três primeiras de onda têm o sentido da solta.
    /// As quatro por entregar vão ao classificador, e a de onda que voltou
    /// entra como entregue. Com `--apply`, a solta ganha a versão nova com a
    /// de onda parada e a sem agente, que saem; a de agente vivo fica como
    /// está, com aviso; e a onda parada e a sem agente, sem tarefa, saem do ar.
    #[test]
    #[cfg(target_os = "linux")]
    fn backlog_triage_sees_every_unfinished_task_and_writes_all_but_the_live_wave() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut ids = backlog(root, &[("A retomada lista as ondas no ar", "apps/rt/src/commands/flow/resume.rs", &[])]);
        let seed = |event_type: &str, body: Value| crate::shared::spec_state::seed_event(root, "x", event_type, body);
        let (live, closed) = (crate::commands::flow::stuck::this_process(), crate::commands::flow::round::closed_process());
        let waves: [(&str, &str, (u32, u64), Option<Value>); 4] = [
            ("A retomada diz a onda que perdeu o processo", "web/stuck.ts", live, Some(json!({ "text": "Parei.", "replan": "Outra tarefa antes." }))),
            ("A retomada mostra a onda sem agente", "src/Resume.cs", closed, None),
            ("A retomada avisa a onda com agente", "web/live.ts", live, None),
            ("A retomada conta as ondas entregues", "web/count.ts", live, Some(json!({ "text": "Feito." }))),
        ];
        let crit = seed("criterion", json!({ "when": "a onda roda", "then": "a suíte passa", "proof": "git --version", "form": "ubiquitous" }));
        let said = seed("message", json!({ "author": "user", "text": "o plano" }));
        for (n, (title, file, sender, back)) in (1u64..).zip(waves) {
            seed("wave", json!({ "n": n, "text": format!("Onda {n}."), "criteria": [crit], "done_when": "A suíte passa.", "author": "binary" }));
            ids.push(seed("task", json!({ "wave": n, "title": title, "text": format!("{title}."), "files": [{ "path": file }], "depends_on": [],
                "origin": said })));
            let mut send = json!({ "wave": n, "role": "wave", "text": "pedido", "lines": 1, "chars": 6, "items": [], "mustard": "0", "author": "binary" });
            send.as_object_mut().unwrap().extend(crate::commands::flow::round::sender_fields(Some(sender)));
            seed("send", send);
            if let Some(mut back) = back {
                back["wave"] = json!(n);
                back["returned"] = json!(true);
                seed("delivered", back);
            }
        }
        let answer = grouped(&[(ids[1], ids[0]), (ids[2], ids[0]), (ids[3], ids[0])]);
        let asked = std::cell::RefCell::new(Vec::new());
        let ask = |_: &Path, _: &str, payload: &str| {
            asked.borrow_mut().push(serde_json::from_str::<Value>(payload).unwrap());
            Some(answer.clone())
        };
        let opts = |apply| TriageOpts { root: root.to_path_buf(), spec: Some("x".into()), apply };

        let shown = triage_with(&opts(false), None, &ask);
        let state = asked.borrow()[0]["state"].clone();
        let mut open: Vec<String> = state["tasks"].as_object().unwrap().keys().cloned().collect();
        open.sort();
        let mut expected: Vec<String> = ids[..4].iter().map(|id| format!("t{id}")).collect();
        expected.sort();
        assert_eq!(open, expected, "every unfinished task goes to the classifier: {shown}");
        assert!(state["delivered"].get(format!("d{}", ids[4])).is_some(), "the returned wave counts as delivered: {state}");
        assert_eq!((shown["classified"].clone(), shown["kept_out"].clone()), (json!(4), json!(1)), "{shown}");
        assert!(shown["warnings"][0].as_str().is_some_and(|said| said.contains("MSTD-TASK-0004")), "{shown}");

        let applied = triage_with(&opts(true), None, &ask);
        assert_eq!(applied["applied"], json!(true), "{applied}");
        assert_eq!(applied["waves_left_without_tasks"], json!([1, 2]), "{applied}");
        let log = log_of(root);
        let visible: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "task").collect();
        let kept = visible.iter().find(|e| e.int("replaces") == Some(ids[0])).expect("the new version of the loose task");
        assert_eq!(kept.wave(), None, "{kept:?}");
        assert!(task_files(kept).contains("web/stuck.ts") && task_files(kept).contains("src/Resume.cs"), "{kept:?}");
        assert!(!task_files(kept).contains("web/live.ts"), "the live wave task is not written: {kept:?}");
        assert!(visible.iter().all(|e| e.id != ids[1] && e.id != ids[2]), "the stopped and the agentless tasks leave their waves");
        assert!(visible.iter().any(|e| e.id == ids[3] && e.wave() == Some(3)), "the live wave task stays as it is");
        assert_eq!(waves_left_without_tasks(&log).into_keys().collect::<Vec<_>>(), vec![1, 2], "the waves without tasks leave the air");
    }

    /// Dois grupos: o primeiro passa; o segundo junta à terceira a quinta,
    /// que depende da quarta, que depende da terceira, e a versão nova
    /// fecharia o círculo. A recusa desfaz o primeiro grupo, e nada fica.
    #[test]
    fn backlog_triage_records_nothing_when_a_write_in_the_middle_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let ids = backlog(root, &[
            ("A retomada lista as ondas no ar", "apps/rt/src/commands/flow/resume.rs", &[]),
            ("A retomada diz a onda que perdeu o processo", "apps/rt/src/commands/flow/stuck.rs", &[]),
            ("A busca não espera a trava do mapa", "apps/rt/src/commands/search.rs", &[]),
            ("A busca lê o mapa sem esperar", "apps/rt/src/commands/map.rs", &[2]),
            ("A busca responde o que foi pedido", "apps/rt/src/commands/search_cli.rs", &[3]),
        ]);
        let answer = grouped(&[(ids[1], ids[0]), (ids[4], ids[2])]);
        let events = log_of(root).events.len();
        let opts = TriageOpts { root: root.to_path_buf(), spec: Some("x".into()), apply: true };
        let refused = triage_with(&opts, None, &|_, _, _| Some(answer.clone()));
        assert_eq!(refused["reason"], json!("task-dependency-cycle"), "{refused}");
        assert_eq!(log_of(root).events.len(), events, "a refusal in the middle records nothing");
    }
}
