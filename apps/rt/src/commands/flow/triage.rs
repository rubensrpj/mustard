//! `mustard-rt run triage --spec <spec>` — reorganiza a lista aberta da spec
//! em quatro passos: o programa lê as tarefas; o modelo pequeno da própria
//! sessão lê a lista inteira de uma vez, separa por assunto e junta as iguais
//! ou próximas ([`task_triage::grouping_request`]); o Jev só ordena as
//! tarefas de cada assunto e os assuntos ([`task_triage::order_backlog_with`]).
//!
//! A primeira chamada devolve o pedido ao agente de análise da sessão, em
//! `grouping`; o recibo dele volta por `run knowledge --complete-session`, e
//! a chamada seguinte, com a mesma lista, acha a leitura pronta e pede a
//! ordem ao Jev. Sem `--apply`, nada é gravado: a resposta mostra os assuntos
//! na ordem, as tarefas de cada um na ordem e as que viram uma só. Com
//! `--apply`, sob a trava do arquivo de eventos e tudo ou nada, a tarefa que
//! fica ganha a versão nova com o trabalho das que entram nela numa linha da
//! parte do agente, as outras saem com o motivo, e cada tarefa ganha o
//! assunto e a posição nele (`group`); a resposta traz, para cada versão
//! nova, o pedido ao agente de análise da sessão que escreve o texto dela
//! ([`task_triage::rewrite_request`]). Vai à leitura toda tarefa ainda por
//! entregar, com onda ou sem; a de onda que voltou sem pedir mudança de plano
//! conta como entregue e fica de fora. A gravação mexe também na tarefa de
//! onda parada ou sem agente, que sai da onda na versão nova, e a onda que
//! fica sem tarefa sai do ar; só a tarefa de onda com agente vivo — lançado
//! depois do envio e com sinal de vida recente — fica de fora da gravação,
//! com aviso. A onda solta que ninguém lançou não segura tarefa nenhuma.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState};
use mustard_core::io::knowledge::enrichment::{GroupingSummary, GroupingTask, WaveGroup, completed_grouping};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use crate::commands::flow::round::queue::{
    backlog_population, max_parallel, open_sends, task_files, tasks_not_delivered, waves_returned, waves_returned_without_replan, waves_with_live_agent,
};
use crate::commands::flow::round::{append_agent_line, backlog_return, is_cleanup, waves_left_without_tasks};
use crate::commands::spec_events::{self, write::record_locked};
use crate::hooks::session::conversation_size::WAVE_LIMIT;
use crate::shared::spec_state::{DiskSpecState, checkout, session_from_env};
use crate::shared::task_triage::{self, BacklogOrder, Candidate, Excerpt, OrderFacts, OrderedSubject};

/// Options for `mustard-rt run triage`.
pub struct TriageOpts {
    pub root: PathBuf,
    pub spec: Option<String>,
    /// Grava a reorganização em vez de só mostrá-la.
    pub apply: bool,
}

/// O relatório do `run triage`, com a pergunta ao Jev da orquestração, que
/// espera a resposta o prazo da lista inteira.
pub(crate) fn triage_at(opts: &TriageOpts) -> Value {
    triage_with(opts, session_from_env().as_deref(), false, &|root, spec, payloads| task_triage::backlog_jev_ask(root, spec)(payloads))
}

/// O que a formação das ondas deu dentro da rodada.
#[derive(Debug, Default)]
pub(crate) struct RoundForming {
    /// O pedido ao agente de análise da sessão que ainda espera o recibo:
    /// enquanto ele não volta, a rodada não monta onda nova da lista.
    pub pending: Option<Value>,
    /// As ondas formadas se gravaram, e a spec precisa ser relida.
    pub applied: bool,
}

/// A formação das ondas pela rodada da spec `spec` de `root`, na sessão
/// `session` ([`triage_with`], com a gravação): na aprovação, com a lista
/// inteira; depois, com as tarefas que voltaram e as que nenhuma onda levou.
/// Só forma com uma sessão conduzindo, porque a leitura é do modelo pequeno
/// dela; sem sessão, sem nada a ler ou sem vaga, nada muda, e a rodada monta
/// as ondas como sempre.
pub(crate) fn form_for_round(root: &Path, spec: &str, session: Option<&str>) -> RoundForming {
    if session.is_none() {
        return RoundForming::default();
    }
    let opts = TriageOpts { root: root.to_path_buf(), spec: Some(spec.into()), apply: true };
    let report = triage_with(&opts, session, true, &|root, spec, payloads| task_triage::backlog_jev_ask(root, spec)(payloads));
    if report.get("grouping").is_some() {
        return RoundForming { pending: Some(json!({ "grouping": report["grouping"], "next": report["next"] })), applied: false };
    }
    RoundForming { pending: None, applied: report["applied"] == json!(true) }
}

/// `true` quando a tarefa `task` está numa onda que o classificador formou
/// e que ainda não voltou com resumo: ela não se relê.
fn formed(task: &SpecEvent) -> bool {
    task.fields.get("group").and_then(|group| group.get("formed")).and_then(Value::as_bool) == Some(true)
}

/// [`triage_at`] com a sessão recebida e a pergunta feita por `ask`, que
/// recebe a raiz do projeto, a spec e os pedidos, e devolve as respostas na
/// ordem deles.
/// `in_round` é a formação dentro da rodada: ela nunca lê nem grava tarefa de
/// onda em andamento, tenha o agente dela começado ou não, e cada uma dessas
/// ondas toma uma vaga. Fora dela, no comando, só a onda com agente vivo
/// segura as tarefas, e a parada sem agente as solta.
fn triage_with(opts: &TriageOpts, session: Option<&str>, in_round: bool, ask: &dyn Fn(&Path, &str, &[String]) -> Option<Vec<Value>>) -> Value {
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
    // dela entram como entregues. A de agente vivo — lançado depois do envio
    // e com sinal de vida recente — entra na classificação e fica de fora da
    // gravação; a solta sem agente lançado não segura nada.
    let delivered = waves_returned_without_replan(&log);
    let live: BTreeSet<u64> = if in_round {
        let returned = waves_returned(&log);
        open_sends(&log).into_keys().filter(|wave| !returned.contains(wave)).collect()
    } else {
        waves_with_live_agent(&project.root, &spec, &log).into_keys().collect()
    };
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
    // 1. Ler só o que importa: as tarefas que ainda não estão numa onda com
    // agente trabalhando — na aprovação, a lista inteira; depois, as que
    // voltaram e as que nenhuma onda levou — e os resumos que ainda valem. A
    // tarefa de onda viva não se relê.
    // A tarefa de uma onda já formada só volta à leitura quando a onda a
    // devolve com resumo — um resumo mais novo que a versão em que ela foi
    // posta na onda; o resumo de antes da formação não a traz de volta. Até
    // lá ela espera a vez numa vaga.
    let mut carried: BTreeMap<u64, u64> = BTreeMap::new();
    for (summary, left) in log.live_summaries() {
        for task in left {
            let newest = carried.entry(task).or_default();
            *newest = (*newest).max(summary.id);
        }
    }
    let waiting = |id: &u64| log.get(*id).is_some_and(|task| formed(task) && carried.get(id).is_none_or(|summary| *summary < task.id));
    let open: Vec<Candidate> = classified.iter().filter(|id| !held.contains(id) && !waiting(id)).filter_map(|id| log.get(*id)).map(candidate).collect();
    let formed_waiting: BTreeSet<String> = classified
        .iter()
        .filter(|id| !held.contains(id) && waiting(id))
        .filter_map(|id| log.get(*id)?.fields.get("group")?.get("name")?.as_str().map(str::to_string))
        .collect();
    let mut warnings: Vec<String> = Vec::new();
    if !held.is_empty() {
        let tasks: Vec<String> = held.iter().map(|id| code(*id)).collect();
        warnings.push(translate("triage.held", lang).replace("{tasks}", &tasks.join(", ")));
    }
    let mut report = json!({ "ok": true, "spec": spec, "applied": false, "classified": open.len(), "kept_out": held.len() });
    let finish = |mut report: Value, warnings: &[String]| {
        if !warnings.is_empty() {
            report["warnings"] = json!(warnings);
        }
        report
    };
    let reading_ids: BTreeSet<u64> = open.iter().map(|task| task.id).collect();
    let summaries: Vec<GroupingSummary> = log
        .live_summaries()
        .into_iter()
        .filter_map(|(summary, left)| {
            let tasks: Vec<String> = left.iter().filter(|id| reading_ids.contains(id)).map(|id| code(*id)).collect();
            (!tasks.is_empty()).then(|| GroupingSummary { code: code(summary.id), text: summary.str_field("text").unwrap_or_default().into(), tasks })
        })
        .collect();
    // As vagas livres: as do projeto menos as das ondas com agente vivo e as
    // das ondas já formadas que esperam a vez.
    let max_waves = max_parallel(&project.root).saturating_sub(live.len() + formed_waiting.len());
    report["free_waves"] = json!(max_waves);
    report["summaries"] = json!(summaries.len());
    // Uma tarefa sozinha, sem resumo, já é a própria onda: não há o que ler
    // junto nem o que juntar.
    if open.len() + summaries.len() < 2 || max_waves == 0 {
        report["waves"] = json!([]);
        return finish(report, &warnings);
    }
    // 2. O modelo pequeno da sessão lê tudo isso de uma vez e forma até
    // `max_waves` ondas, cada uma com um objetivo, juntando as tarefas iguais
    // ou próximas; a leitura dele, quando já voltou para esta mesma lista, dá
    // as ondas.
    let tasks: Vec<GroupingTask> = open
        .iter()
        .map(|task| GroupingTask { code: code(task.id), title: task.title.into(), text: task.text.into(), files: task.files.iter().cloned().collect() })
        .collect();
    let request = task_triage::grouping_request(&project.root, &tasks, &summaries, max_waves);
    if let Some(reason) = request.get("unavailable") {
        report["miss"] = reason.clone();
        return finish(report, &warnings);
    }
    report["read_chars"] = json!(request["delegation"]["request"]["prompt"].as_str().map_or(0, str::len));
    let Some(formed) = completed_grouping(&project.root, request["request_id"].as_str().unwrap_or_default()) else {
        report["grouping"] = request;
        report["next"] = json!(translate("triage.next_grouping", lang));
        return finish(report, &warnings);
    };
    let by_id: BTreeMap<u64, &Candidate> = open.iter().map(|task| (task.id, task)).collect();
    let by_code: BTreeMap<String, u64> = open.iter().map(|task| (code(task.id), task.id)).collect();
    let id_of = |said: &String| by_code.get(said).copied();
    let continued = |wave: &WaveGroup| summaries.iter().find(|summary| summary.tasks.iter().any(|left| wave.tasks.contains(left))).map(|summary| summary.code.clone());
    // O objetivo de cada onda é o nome do grupo e é único: a rodada junta
    // numa onda as tarefas de mesmo grupo.
    let mut named: BTreeMap<String, usize> = BTreeMap::new();
    let subjects: Vec<OrderedSubject> = formed
        .iter()
        .map(|wave| {
            let absorbed: BTreeSet<&String> = wave.merges.iter().flat_map(|merge| &merge.absorb).collect();
            let joined = |keep: &String| -> Vec<u64> {
                wave.merges.iter().find(|merge| &merge.keep == keep).map(|merge| merge.absorb.iter().filter_map(id_of).collect()).unwrap_or_default()
            };
            let tasks = wave.tasks.iter().filter(|said| !absorbed.contains(said)).filter_map(|said| Some((id_of(said)?, joined(said)))).collect();
            let seen = named.entry(wave.objective.clone()).or_default();
            *seen += 1;
            let name = if *seen == 1 { wave.objective.clone() } else { format!("{} ({seen})", wave.objective) };
            OrderedSubject { name, tasks }
        })
        .collect();
    let continues: BTreeMap<String, String> =
        formed.iter().zip(&subjects).filter_map(|(wave, subject)| Some((subject.name.clone(), continued(wave)?))).collect();
    let placed: BTreeSet<u64> = formed.iter().flat_map(|wave| wave.tasks.iter().filter_map(id_of)).collect();
    let later: Vec<String> = open.iter().filter(|task| !placed.contains(&task.id)).map(|task| code(task.id)).collect();
    // 3. O programa confere se cada onda cabe no limite de uma onda, pelos
    // trechos que o agente lê: a declaração que a tarefa cita pelo nome, o
    // resumo do arquivo em que ela não cita nenhuma e o arquivo que o mapa não
    // conhece, inteiro.
    let map = mustard_core::io::project_map::read(&project.root).ok();
    let module = |file: &str| map.as_ref().and_then(|map| map.modules.iter().find(|module| module.path == file));
    let declared = |file: &str| -> Option<Vec<task_triage::Declared>> {
        module(file).map(|module| {
            module.declarations.iter().map(|it| task_triage::Declared { name: it.name.clone(), line: it.line, end_line: it.end_line }).collect()
        })
    };
    let disk = |file: &str| std::fs::metadata(project.root.join(file)).map_or(0, |meta| meta.len() / BYTES_PER_TOKEN);
    let cost = |excerpt: &Excerpt| -> u64 {
        match excerpt {
            Excerpt::Whole(file) => disk(file).max(1),
            Excerpt::Summary(file) => module(file)
                .map_or(1, |module| module.declarations.iter().map(|it| it.signature.len() as u64 / BYTES_PER_TOKEN + SUMMARY_LINE_TOKENS).sum()),
            Excerpt::Declaration(file, line) => module(file).map_or(1, |module| {
                let per_line = (disk(file) / (module.loc as u64).max(1)).max(1);
                module.declarations.iter().find(|it| it.line == *line).map_or(1, |it| (it.end_line.saturating_sub(it.line) + 1) * per_line)
            }),
        }
    };
    let said = |id: &u64| {
        log.get(*id)
            .map(|task| ["title", "text", "agent"].map(|field| task.str_field(field).unwrap_or_default()).join(" "))
            .unwrap_or_default()
    };
    let excerpts = |ids: &mut dyn Iterator<Item = &u64>| -> BTreeSet<Excerpt> {
        ids.filter_map(|id| by_id.get(id).map(|task| task_triage::excerpts_of(&task_triage::code_names(&said(id)), &task.files, &declared))).flatten().collect()
    };
    let reading = |excerpts: &BTreeSet<Excerpt>| -> u64 { excerpts.iter().map(cost).sum() };
    let jev = |payloads: &[String]| ask(&project.root, &spec, payloads);
    // 3. A ordem parte do que o programa já sabe, da mesma fonte da fila; o
    // Jev só desempata.
    let facts: BTreeMap<u64, OrderFacts> = backlog_population(&log, &BTreeSet::new())
        .into_iter()
        .filter(|task| classified.contains(&task.id))
        .map(|task| {
            let cleanup = log.get(task.id).is_some_and(is_cleanup);
            (task.id, OrderFacts { priority: task.priority, cleanup, depends_on: task.depends_on })
        })
        .collect();
    let order = task_triage::order_backlog_with(&open, subjects, &facts, &jev);
    if order.unanswered > 0 {
        warnings.push(
            translate("triage.order_unanswered", lang).replace("{missing}", &order.unanswered.to_string()).replace("{requests}", &order.requests.to_string()),
        );
    }
    let shown: Vec<Value> = order
        .subjects
        .iter()
        .map(|subject| {
            let merges: Vec<Value> = subject
                .tasks
                .iter()
                .filter(|(_, joined)| !joined.is_empty())
                .map(|(keep, joined)| json!({ "keep": code(*keep), "join": joined.iter().map(|id| code(*id)).collect::<Vec<_>>() }))
                .collect();
            let read = reading(&excerpts(&mut subject.tasks.iter().flat_map(|(keep, joined)| std::iter::once(keep).chain(joined))));
            json!({ "objective": subject.name, "tasks": subject.tasks.iter().map(|(keep, _)| code(*keep)).collect::<Vec<_>>(), "merges": merges,
                "continues": continues.get(&subject.name), "reading_tokens": read })
        })
        .collect();
    for wave in shown.iter().filter(|wave| wave["reading_tokens"].as_u64().is_some_and(|read| read > WAVE_LIMIT)) {
        warnings.push(
            translate("triage.wave_too_big", lang)
                .replace("{objective}", wave["objective"].as_str().unwrap_or_default())
                .replace("{tokens}", &wave["reading_tokens"].to_string()),
        );
    }
    // A leitura que a onda economiza: cada tarefa sozinha lê os trechos
    // dela; na onda, o trecho em comum se lê uma vez.
    let alone: u64 = order
        .subjects
        .iter()
        .flat_map(|subject| subject.tasks.iter().flat_map(|(keep, joined)| std::iter::once(keep).chain(joined)))
        .map(|id| reading(&excerpts(&mut std::iter::once(id))))
        .sum();
    let grouped: u64 = shown.iter().filter_map(|wave| wave["reading_tokens"].as_u64()).sum();
    report["waves"] = json!(shown);
    report["later"] = json!(later);
    report["reading_tokens"] = json!({ "each_task_alone": alone, "in_waves": grouped });
    report["requests"] = json!(order.requests);
    let mut report = finish(report, &warnings);
    if !opts.apply {
        report["next"] = json!(translate("triage.next_dry", lang));
        return report;
    }
    // 4. A gravação: as junções e o assunto de cada tarefa.
    match apply(&opts.root, &spec, &path, &order, &held, lang) {
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

/// Quantos bytes de um arquivo valem um token de leitura, na conta do custo
/// de cada trecho.
const BYTES_PER_TOKEN: u64 = 4;

/// Os tokens de cada linha do resumo de um arquivo além da assinatura: o
/// tipo, o nome e a linha da declaração.
const SUMMARY_LINE_TOKENS: u64 = 5;

/// O que [`apply`] gravou: o número de cada versão nova, com as tarefas do
/// grupo que entraram nela, a primeira à frente, e as ondas que ficaram sem
/// tarefa pela gravação e saíram do ar.
struct Applied {
    versions: Vec<(u64, Vec<u64>)>,
    emptied: Vec<u64>,
}

/// Grava a lista ordenada `order` na spec `spec`, vista de `start`, sob a
/// trava do arquivo de eventos `path`, sem tocar nas tarefas de `held`, de
/// onda com agente vivo: em cada assunto, na ordem, a tarefa que fica ganha a
/// versão nova, sem onda, com o assunto e a posição nele (`group`); a que
/// recebe outras ganha nela o trabalho delas ([`joined_version`]), e as que
/// entram saem, com o motivo no idioma `lang`. A tarefa que já está no
/// assunto e na posição, sem nada a receber, não grava nada. Uma recusa no
/// meio volta o arquivo ao que era antes da primeira gravação, e nada fica.
fn apply(start: &Path, spec: &str, path: &Path, order: &BacklogOrder, held: &BTreeSet<u64>, lang: Locale) -> Result<Applied, Refusal> {
    let locked_result = store::with_locked_writer(path, |locked| {
        let before = locked.content().to_string();
        let in_air = waves_left_without_tasks(locked.log());
        let mut record_all = || -> Result<Applied, Refusal> {
            let mut versions = Vec::new();
            for subject in &order.subjects {
                for (position, (keep, joined)) in (1u64..).zip(&subject.tasks) {
                    if held.contains(keep) {
                        continue;
                    }
                    let group: Vec<u64> = std::iter::once(*keep).chain(joined.iter().copied().filter(|id| !held.contains(id))).collect();
                    let place = json!({ "name": subject.name, "position": position, "formed": true });
                    let mut draft = if group.len() > 1 {
                        joined_version(locked.log(), &group, lang)?
                    } else {
                        let task = locked.log().get(*keep).ok_or(Refusal::UnknownTarget { target: mustard_core::domain::spec_events::EventRef::Id(*keep) })?;
                        if task.fields.get("group") == Some(&place) {
                            continue;
                        }
                        backlog_return(task)
                    };
                    draft.insert("group".into(), place);
                    let version = record_locked(locked, start, spec, "task", draft, PhaseWriter::Binary)?.written.id;
                    if group.len() > 1 {
                        let keeper = locked.log().codes().get(keep).cloned().unwrap_or_else(|| keep.to_string());
                        let removal = json!({ "targets": group[1..], "reason": translate("triage.removed_joined", lang).replace("{code}", &keeper) });
                        record_locked(locked, start, spec, "remove", object(removal), PhaseWriter::Binary)?;
                        versions.push((version, group));
                    }
                }
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

    /// O recibo do modelo pequeno da sessão para o pedido de leitura que a
    /// resposta `shown` trouxe, com as ondas `waves`, entregue pela
    /// mesma porta de `run knowledge --complete-session`.
    fn read_by_the_session(root: &Path, shown: &Value, waves: Value) {
        let receipt = mustard_core::io::knowledge::enrichment::SessionReceipt {
            request_id: shown["grouping"]["request_id"].as_str().unwrap_or_else(|| panic!("{shown}")).into(),
            model: "inherit".into(),
            effort: None,
            result: json!({ "waves": waves }),
            input_tokens: None,
            output_tokens: None,
        };
        mustard_core::io::knowledge::enrichment::complete_session(root, root, &receipt).unwrap();
    }

    /// O Jev falso: o sim de 0,9 a cada junção, a nota de cada pergunta de
    /// `levels`, e o nível 1 nas outras notas.
    fn scored(levels: &[(&str, u8)]) -> impl Fn(&Path, &str, &[String]) -> Option<Vec<Value>> + use<> {
        let levels: Vec<(String, u8)> = levels.iter().map(|(key, level)| (key.to_string(), *level)).collect();
        move |_, _, payloads| {
            let answer = |payload: &String| {
                let doc: Value = serde_json::from_str(payload).unwrap();
                let answers: Map<String, Value> = doc["questions"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(|key| {
                        if key.starts_with("join_") {
                            return (key.clone(), json!({ "type": "noul", "noul": 0.9 }));
                        }
                        let level = levels.iter().find(|(asked, _)| asked == key).map_or(1, |(_, level)| *level);
                        (key.clone(), json!({ "type": "score", "score": f64::from(level), "probabilities": { level.to_string(): 0.9 } }))
                    })
                    .collect();
                json!({ "answers": answers })
            };
            Some(payloads.iter().map(answer).collect())
        }
    }

    /// Quatro tarefas de um projeto em TypeScript: a retomada que lista as
    /// ondas no ar, a busca que não espera a trava do mapa, a retomada que diz
    /// a onda que perdeu o processo e o título do commit. A primeira chamada
    /// só devolve o pedido ao modelo pequeno da sessão, com a lista inteira e
    /// as quatro vagas livres, e não pergunta nada ao Jev. Ele forma duas
    /// ondas — a retomada, com a terceira dentro da primeira, e a busca — e
    /// deixa o commit para depois. O Jev ordena as ondas: a busca vale mais.
    /// Sem `--apply` nada se grava; com `--apply`, a primeira ganha a versão
    /// nova com a terceira e o objetivo da onda, a terceira sai, a busca ganha
    /// o objetivo dela, o commit fica como está, e a resposta traz o pedido de
    /// reescrita.
    #[test]
    fn backlog_triage_forms_waves_by_objective_and_orders_with_jev() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let ids = backlog(root, &[
            ("A retomada lista as ondas no ar", "web/flow/resume.ts", &[]),
            ("A busca não espera a trava do mapa", "web/search/index.ts", &[]),
            ("A retomada diz a onda que perdeu o processo", "web/flow/stuck.ts", &[]),
            ("O commit leva título e corpo", "web/round/commit.ts", &[]),
        ]);
        let opts = |apply| TriageOpts { root: root.to_path_buf(), spec: Some("x".into()), apply };
        let events = log_of(root).events.len();

        let asked = triage_with(&opts(false), None, false, &|_, _, _| panic!("the Jev only orders, after the reading"));
        let prompt = asked["grouping"]["delegation"]["request"]["prompt"].as_str().unwrap_or_else(|| panic!("{asked}"));
        assert!(prompt.contains("A retomada lista as ondas no ar") && prompt.contains("O commit leva título e corpo"), "the whole list in one request");
        assert!(prompt.contains("Form up to 4 waves"), "the free slots: {prompt}");
        assert_eq!(asked["grouping"]["delegation"]["model_family"], json!("haiku"), "{asked}");
        read_by_the_session(root, &asked, json!([
            { "objective": "A retomada mostra as ondas no ar", "tasks": ["MSTD-TASK-0001", "MSTD-TASK-0003"],
              "merges": [{ "keep": "MSTD-TASK-0001", "absorb": ["MSTD-TASK-0003"] }] },
            { "objective": "A busca responde sem esperar a trava", "tasks": ["MSTD-TASK-0002"], "merges": [] },
        ]));

        let jev = scored(&[("value_s0", 1), ("value_s1", 3)]);
        let shown = triage_with(&opts(false), None, false, &jev);
        let objectives: Vec<&str> = shown["waves"].as_array().unwrap().iter().map(|wave| wave["objective"].as_str().unwrap()).collect();
        assert_eq!(objectives, ["A busca responde sem esperar a trava", "A retomada mostra as ondas no ar"], "{shown}");
        assert_eq!(shown["waves"][1]["merges"], json!([{ "keep": "MSTD-TASK-0001", "join": ["MSTD-TASK-0003"] }]), "{shown}");
        assert_eq!(shown["later"], json!(["MSTD-TASK-0004"]), "the task left out waits for a later forming: {shown}");
        assert_eq!(shown["requests"], json!(1), "only the order of the waves; a wave of one task asks nothing about tasks: {shown}");
        assert_eq!(log_of(root).events.len(), events, "nothing is recorded without --apply");

        let applied = triage_with(&opts(true), None, false, &jev);
        assert_eq!(applied["applied"], json!(true), "{applied}");
        let log = log_of(root);
        let visible: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "task").collect();
        assert_eq!(visible.len(), 3, "{visible:?}");
        let kept = visible.iter().find(|e| e.int("replaces") == Some(ids[0])).expect("the new version of the first");
        assert!(kept.str_field("agent").unwrap().contains("MSTD-TASK-0003"), "{kept:?}");
        assert!(task_files(kept).contains("web/flow/stuck.ts"));
        assert_eq!(kept.fields["group"], json!({ "name": "A retomada mostra as ondas no ar", "position": 1, "formed": true }));
        let search = visible.iter().find(|e| e.int("replaces") == Some(ids[1])).expect("the search gets its wave");
        assert_eq!(search.fields["group"], json!({ "name": "A busca responde sem esperar a trava", "position": 1, "formed": true }));

        // A formação seguinte só lê o commit, que nenhuma onda levou: as das
        // duas ondas formadas esperam a vez, e cada uma toma uma vaga. Sozinho,
        // o commit já é a própria onda, e o Haiku nem é chamado.
        let next = triage_with(&opts(false), None, false, &|_, _, _| panic!("nothing to order"));
        assert_eq!((next["classified"].clone(), next["free_waves"].clone()), (json!(1), json!(2)), "{next}");
        assert!(next.get("grouping").is_none() && next["waves"] == json!([]), "{next}");
        assert!(visible.iter().any(|e| e.id == ids[3] && !e.fields.contains_key("group")), "the task left for later stays as it is");
        assert!(visible.iter().all(|e| e.id != ids[2]), "the joined task leaves");
        assert!(applied["rewrite"][0]["request"]["delegation"]["model_family"].is_string(), "{applied}");
    }

    /// Uma tarefa solta e cinco de onda: a de onda parada por mudança de
    /// plano, à espera da resposta; a de onda sem agente, cujo envio é de um
    /// processo que já fechou; a de onda com agente vivo, lançado depois do
    /// envio do processo deste teste; a de onda que voltou sem mudança de
    /// plano, à espera só da rodada; e a de onda solta pelo processo deste
    /// teste sem agente lançado. O modelo pequeno da sessão põe as de onda por
    /// entregar dentro da solta. As cinco por entregar vão à leitura, e a de
    /// onda que voltou fica de fora, como entregue. Com `--apply`, a solta ganha a
    /// versão nova com a de onda parada, a sem agente e a solta sem
    /// lançamento, que saem; a de agente vivo fica como está, com aviso; e as
    /// três ondas que ficam sem tarefa saem do ar.
    #[test]
    #[cfg(target_os = "linux")]
    fn backlog_triage_sees_every_unfinished_task_and_writes_all_but_the_live_wave() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut ids = backlog(root, &[("A retomada lista as ondas no ar", "apps/rt/src/commands/flow/resume.rs", &[])]);
        let seed = |event_type: &str, body: Value| crate::shared::spec_state::seed_event(root, "x", event_type, body);
        let (live, closed) = (crate::commands::flow::stuck::this_process(), crate::commands::flow::round::closed_process());
        let waves: [(&str, &str, (u32, u64), bool, Option<Value>); 5] = [
            ("A retomada diz a onda que perdeu o processo", "web/stuck.ts", live, true, Some(json!({ "text": "Parei.", "replan": "Outra tarefa antes." }))),
            ("A retomada mostra a onda sem agente", "src/Resume.cs", closed, true, None),
            ("A retomada avisa a onda com agente", "web/live.ts", live, true, None),
            ("A retomada conta as ondas entregues", "web/count.ts", live, true, Some(json!({ "text": "Feito." }))),
            ("A retomada lista a onda solta", "web/sent.ts", live, false, None),
        ];
        let crit = seed("criterion", json!({ "when": "a onda roda", "then": "a suíte passa", "proof": "git --version", "form": "ubiquitous" }));
        let said = seed("message", json!({ "author": "user", "text": "o plano" }));
        for (n, (title, file, sender, launched, back)) in (1u64..).zip(waves) {
            seed("wave", json!({ "n": n, "text": format!("Onda {n}."), "criteria": [crit], "done_when": "A suíte passa.", "author": "binary" }));
            ids.push(seed("task", json!({ "wave": n, "title": title, "text": format!("{title}."), "files": [{ "path": file }], "depends_on": [],
                "origin": said })));
            let mut send = json!({ "wave": n, "role": "wave", "text": "pedido", "lines": 1, "chars": 6, "items": [], "mustard": "0", "author": "binary" });
            send.as_object_mut().unwrap().extend(crate::commands::flow::round::sender_fields(Some(sender)));
            if launched {
                send.as_object_mut().unwrap().extend(crate::commands::flow::round::queue::launch_fields());
            }
            seed("send", send);
            if let Some(mut back) = back {
                back["wave"] = json!(n);
                back["returned"] = json!(true);
                seed("delivered", back);
            }
        }
        let ask = scored(&[]);
        let opts = |apply| TriageOpts { root: root.to_path_buf(), spec: Some("x".into()), apply };

        let asked = triage_with(&opts(false), None, false, &ask);
        let prompt = asked["grouping"]["delegation"]["request"]["prompt"].as_str().unwrap_or_else(|| panic!("{asked}")).to_string();
        let title = |id: u64| log_of(root).get(id).unwrap().str_field("title").unwrap().to_string();
        for id in [ids[0], ids[1], ids[2], ids[5]] {
            assert!(prompt.contains(&title(id)), "every unfinished task outside a live wave goes to the reading: {}", title(id));
        }
        assert!(!prompt.contains(&title(ids[3])), "the task of the live wave is not read again");
        assert!(!prompt.contains(&title(ids[4])), "the returned wave counts as delivered");
        assert!(prompt.contains("Form up to 3 waves"), "four slots, one held by the live wave: {prompt}");
        assert_eq!((asked["classified"].clone(), asked["kept_out"].clone()), (json!(4), json!(1)), "{asked}");
        assert!(asked["warnings"][0].as_str().is_some_and(|said| said.contains("MSTD-TASK-0004")), "{asked}");
        let codes = ["MSTD-TASK-0001", "MSTD-TASK-0002", "MSTD-TASK-0003", "MSTD-TASK-0006"];
        read_by_the_session(root, &asked, json!([{ "objective": "A retomada diz o estado de cada onda", "tasks": codes,
            "merges": [{ "keep": codes[0], "absorb": codes[1..] }] }]));

        let applied = triage_with(&opts(true), None, false, &ask);
        assert_eq!(applied["applied"], json!(true), "{applied}");
        assert_eq!(applied["waves_left_without_tasks"], json!([1, 2, 5]), "{applied}");
        let log = log_of(root);
        let visible: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "task").collect();
        let kept = visible.iter().find(|e| e.int("replaces") == Some(ids[0])).expect("the new version of the loose task");
        assert_eq!(kept.wave(), None, "{kept:?}");
        assert!(task_files(kept).contains("web/stuck.ts") && task_files(kept).contains("src/Resume.cs"), "{kept:?}");
        assert!(task_files(kept).contains("web/sent.ts"), "the task of a wave sent without an agent is written: {kept:?}");
        assert!(!task_files(kept).contains("web/live.ts"), "the live wave task is not written: {kept:?}");
        assert!(visible.iter().all(|e| e.id != ids[1] && e.id != ids[2] && e.id != ids[5]), "the stopped, agentless and unlaunched tasks leave their waves");
        assert!(visible.iter().any(|e| e.id == ids[3] && e.wave() == Some(3)), "the live wave task stays as it is");
        assert_eq!(waves_left_without_tasks(&log).into_keys().collect::<Vec<_>>(), vec![1, 2, 5], "the waves without tasks leave the air");
    }

    /// A rodada forma as ondas só com uma sessão conduzindo. Sem sessão, nada
    /// muda. Com sessão e sem a leitura, ela devolve o pedido ao condutor e
    /// não grava nada; com a leitura pronta, grava as ondas formadas.
    #[test]
    fn the_round_forms_the_waves_only_with_a_session_and_after_the_reading() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        backlog(root, &[("A retomada lista as ondas no ar", "web/flow/resume.ts", &[]), ("A busca não espera a trava do mapa", "web/search/index.ts", &[])]);
        let events = log_of(root).events.len();
        let alone = form_for_round(root, "x", None);
        assert!(alone.pending.is_none() && !alone.applied, "without a session the round assembles as always");
        let asked = form_for_round(root, "x", Some("session"));
        let pending = asked.pending.expect("the request to the session's analysis agent");
        assert!(pending["grouping"]["delegation"]["request"]["prompt"].as_str().is_some_and(|prompt| prompt.contains("A busca")), "{pending}");
        assert_eq!(log_of(root).events.len(), events, "nothing is recorded while the reading is pending");
        read_by_the_session(root, &pending, json!([{ "objective": "A retomada mostra as ondas no ar", "tasks": ["MSTD-TASK-0001"], "merges": [] }]));
        let formed = form_for_round(root, "x", Some("session"));
        assert!(formed.pending.is_none() && formed.applied, "the formed wave is recorded");
        let log = log_of(root);
        assert!(log.visible().iter().any(|e| e.int("replaces").is_some() && e.fields.get("group").and_then(|g| g.get("formed")) == Some(&json!(true))));
    }

    /// Dois assuntos: no primeiro, a segunda entra na primeira e passa; no
    /// segundo, a quinta, que depende da quarta, que depende da terceira,
    /// entra na terceira, e a versão nova fecharia o círculo. A recusa desfaz
    /// o primeiro assunto, e nada fica.
    #[test]
    fn backlog_triage_records_nothing_when_a_write_in_the_middle_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        backlog(root, &[
            ("A retomada lista as ondas no ar", "src/Flow/Resume.cs", &[]),
            ("A retomada diz a onda que perdeu o processo", "src/Flow/Stuck.cs", &[]),
            ("A busca não espera a trava do mapa", "src/Search/Index.cs", &[]),
            ("A busca lê o mapa sem esperar", "src/Search/Map.cs", &[2]),
            ("A busca responde o que foi pedido", "src/Search/Answer.cs", &[3]),
        ]);
        let opts = |apply| TriageOpts { root: root.to_path_buf(), spec: Some("x".into()), apply };
        let jev = scored(&[]);
        let asked = triage_with(&opts(false), None, false, &jev);
        read_by_the_session(root, &asked, json!([
            { "objective": "A retomada diz o estado das ondas", "tasks": ["MSTD-TASK-0001", "MSTD-TASK-0002"],
              "merges": [{ "keep": "MSTD-TASK-0001", "absorb": ["MSTD-TASK-0002"] }] },
            { "objective": "A busca responde sem esperar o mapa", "tasks": ["MSTD-TASK-0003", "MSTD-TASK-0004", "MSTD-TASK-0005"],
              "merges": [{ "keep": "MSTD-TASK-0003", "absorb": ["MSTD-TASK-0005"] }] },
        ]));
        let events = log_of(root).events.len();
        let refused = triage_with(&opts(true), None, false, &jev);
        assert_eq!(refused["reason"], json!("task-dependency-cycle"), "{refused}");
        assert_eq!(log_of(root).events.len(), events, "a refusal in the middle records nothing");
    }
}
