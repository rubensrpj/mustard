//! `mustard-rt run write` — a porta da gravação do modelo pela linha de
//! comando. Ela faz as perguntas ao Jev (a do lugar do pedido novo e a da
//! tarefa que repete uma aberta) e passa o rascunho pelas conferências da
//! rodada: a tarefa só em arquivo que o git ignora, o passo que termina uma
//! tarefa, a volta da onda e o veredito do revisor. A remoção devolve ao
//! backlog as tarefas da onda que ela tira, e a resposta do passo traz a
//! próxima tarefa da onda.
//!
//! A gravação recusada, sozinha ou numa linha do lote, guarda o rascunho dela
//! na pasta da spec e devolve o número em `draft`; a correção que o cita
//! manda só o campo errado ([`super::refusal_draft`]).
//!
//! A gravação em si é a de [`super::write`], a mesma que a rodada e a parte
//! comum do fluxo usam por dentro do binário. A porta importa a rodada e a
//! gravação; a gravação não importa nenhuma das duas, e nada se importa em
//! ciclo.

use std::path::Path;

use mustard_core::domain::lessons::LESSON;
use mustard_core::domain::spec_events::{Refusal, SpecEvent, SpecLog, type_spec};
use mustard_core::domain::spec_index;
use mustard_core::domain::spec_state::{PhaseWriter, SpecState};
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use super::refusal_draft::Resolved;
use super::request_route::Route;
use super::write::{self, Recorded, WriteOpts};
use crate::commands::flow::round::queue::{files_in, task_files};
use crate::commands::flow::round::task_release::{Next, after_step};
use crate::commands::flow::round::{all_ignored, backlog_left, backlog_return, check_return, check_verdict_return, mark_step_copy};
use crate::shared::jev::TASK_TRIAGE_CALL;
use crate::shared::jev::reserve::{self, Asked, RESERVE_FIELD, Reserved};
use crate::shared::spec_state::{DiskSpecState, checkout, session_from_env};
use crate::shared::task_triage::{self, Candidate, Classifying, Destination, Item};

/// As recusas do pedido do modelo que olham só o tipo e os campos
/// ([`write::draft_rules`]), com a da tarefa só em arquivo que o git ignora
/// antes delas: a tarefa assim é mudança local, nenhuma onda a entrega, e o
/// git recusaria o commit dela na volta. A conferência é a do próprio git, a
/// mesma da volta ([`all_ignored`]).
///
/// # Errors
///
/// [`Refusal::FilesIgnoredByGit`], com os arquivos, ou a recusa de
/// [`write::draft_rules`].
pub(crate) fn draft_rules(start: &Path, spec: &str, event_type: &str, draft: &mut Map<String, Value>, returns: bool) -> Result<(), Refusal> {
    // A versão nova de uma tarefa herda o que não mandou antes das
    // conferências: a prova que a parte herdada nomeia, e os arquivos
    // herdados, passam por elas como os mandados.
    if event_type == "task"
        && let Some(log) = DiskSpecState::new(start).log(spec.trim())
    {
        super::task_inherit::inherit_task(&log, true, draft);
    }
    if event_type == "task" {
        let files = files_in(draft);
        if all_ignored(&checkout(start), &files) {
            return Err(Refusal::FilesIgnoredByGit { files: files.into_iter().map(|file| format!("`{file}`")).collect::<Vec<_>>().join(", ") });
        }
    }
    write::draft_rules(start, spec, event_type, draft, returns)
}

/// O núcleo testável de [`run`]: o relatório da gravação ou a recusa, e o
/// `--json` como foi gravado, com o rascunho que ele cita já juntado. Com
/// `copy`, a resposta só avisa que a cópia virou publicação explícita.
/// A tarefa e o pedido novos passam antes pelo classificador das atividades,
/// com a pergunta ao Jev da orquestração ([`same_work_open`]); o pedido novo,
/// ao mesmo tempo, pela pergunta do lugar dele ([`super::request_route`]).
/// A sessão `session` é a que a entrada passou: ela decide a spec da gravação
/// sem `--spec` e vai no evento gravado. Nunca entra em pânico.
pub(crate) fn write_for(opts: &WriteOpts, copy: bool, session: Option<&str>) -> (Value, String) {
    write_joined(opts, copy, session, &|root, spec, payload| reserve::ask_orchestration(root, spec, TASK_TRIAGE_CALL, payload))
}

/// A pergunta ao Jev que [`write_joined`] faz: recebe a raiz do projeto, a
/// spec e o pedido, e diz o que o Jev deu. As perguntas de uma gravação saem
/// ao mesmo tempo, cada uma na sua linha de execução.
type AskJev<'a> = dyn Fn(&Path, &str, &str) -> Asked<Value> + Sync + 'a;

/// As perguntas `payloads` ao Jev por `ask`, todas ao mesmo tempo: a espera
/// é a da mais lenta, não a soma delas. As respostas voltam na ordem.
fn ask_together(root: &Path, spec: &str, payloads: &[String], ask: &AskJev<'_>) -> Vec<Asked<Value>> {
    std::thread::scope(|scope| {
        let running: Vec<_> = payloads.iter().map(|payload| scope.spawn(move || ask(root, spec, payload))).collect();
        running.into_iter().map(|asking| asking.join().unwrap_or(Asked::Failed)).collect()
    })
}

/// Para os testes: o relatório de [`write_joined`], sem o `--json` gravado.
#[cfg(test)]
pub(super) fn write_triaged(opts: &WriteOpts, copy: bool, session: Option<&str>, ask: &AskJev<'_>) -> Value {
    write_joined(opts, copy, session, ask).0
}

/// [`write_for`] com a pergunta ao classificador e ao lugar do pedido feita
/// por `ask` ([`AskJev`]).
fn write_joined(opts: &WriteOpts, copy: bool, session: Option<&str>, ask: &AskJev<'_>) -> (Value, String) {
    // A gravação que cita um rascunho de recusa parte dele, e a recusa desta
    // guarda o rascunho de novo; a que passa o tira.
    let draft = match Resolved::of(&opts.root, opts.spec.as_deref(), session, &opts.event_type, &opts.json) {
        Ok(draft) => draft,
        Err(report) => return (report, opts.json.clone()),
    };
    let joined = WriteOpts { root: opts.root.clone(), spec: opts.spec.clone(), event_type: opts.event_type.clone(), json: draft.json().to_string() };
    let report = match prepare(&joined, session, ask) {
        Ok(mut ready) => {
            let recorded = record_ready(&mut ready, &joined.root, session);
            finish(ready, &joined.root, copy, recorded)
        }
        Err(report) => report,
    };
    (draft.settle(report), joined.json)
}

/// O rascunho que passou por todas as conferências da porta e só falta
/// gravar, com o que o relatório da gravação leva depois dela.
struct Ready {
    project: super::Project,
    spec: String,
    event_type: String,
    draft: Map<String, Value>,
    /// A trava do passo do git que a volta da onda e a do revisor prendem
    /// até estarem no arquivo.
    held: Option<LockedFile>,
    /// Os avisos de antes da gravação: o passo fora da onda, a prova e a
    /// dúvida sobre o lugar do pedido.
    warnings: Vec<String>,
    /// O nome e a ação da skill criada ou atualizada.
    skill: Option<(String, String)>,
    /// A chave do passo seguinte de um pedido, pelo efeito dele.
    next: Option<String>,
}

/// As conferências da porta sobre o pedido `opts`, antes de gravar: o
/// rascunho pronto, ou a resposta que a porta já dá sem gravar na spec — a
/// recusa, a lição e a página do projeto, gravadas fora dela, o pedido que vai
/// a outra spec ou à lista de pendências e a decisão que foi à reserva do Jev.
fn prepare(opts: &WriteOpts, session: Option<&str>, ask: &AskJev<'_>) -> Result<Ready, Value> {
    let project = super::project(&opts.root);
    let lang = project.lang;
    let refuse = move |refusal: Refusal| Err(super::refused(&refusal, lang));

    let mut draft = match write::draft_object(&opts.json) {
        Ok(draft) => draft,
        Err(refusal) => return refuse(refusal),
    };
    // A tarefa ou o pedido gravado de novo depois da reserva do Jev traz o
    // número dela, que decide no lugar do Jev e não fica no evento.
    let reserve_id = if matches!(opts.event_type.trim(), "task" | "request") {
        draft.remove(RESERVE_FIELD).and_then(|id| id.as_str().map(str::to_string))
    } else {
        None
    };
    if draft.get("author").and_then(Value::as_str).map(str::trim) == Some("binary") {
        return refuse(Refusal::BinaryAuthor);
    }
    let event_type = opts.event_type.trim();
    if event_type == LESSON {
        return Err(write::write_lesson(&project, opts.spec.as_deref(), draft));
    }
    let bound;
    let spec = match opts.spec.as_deref() {
        Some(spec) => spec,
        None => {
            // A página do projeto nasce antes de qualquer spec.
            let page = draft.get("page").and_then(Value::as_str).map(str::trim);
            if event_type == "publish" && page == Some(spec_index::PROJECT_PAGE) {
                return Err(write::record_project_page(&project, &draft));
            }
            // Sem spec, um tipo que não existe continua recusado pelo nome; o
            // que existe vai para a spec ligada à conversa que grava.
            if type_spec(event_type).is_none() {
                return refuse(Refusal::UnknownType { found: event_type.to_string() });
            }
            match super::session_spec::of_conversation(&project.root, session, event_type) {
                Ok(spec) => bound = spec,
                Err(refusal) => return refuse(refusal),
            }
            bound.as_str()
        }
    };
    // A spec de antes desta versão do Mustard só se lê: a recusa vem antes de
    // qualquer conferência do item.
    if let Err(refusal) = mustard_core::io::legacy_specs::refuse_legacy(&project.root, spec) {
        return refuse(refusal);
    }
    if matches!(event_type, "task" | "request")
        && let Ok(path) = store::spec_file(&project.root, spec)
            && let Ok(Some(log)) = store::read(&path) {
                let here = opts.root.canonicalize().ok();
                if log.events.iter().filter(|e| e.event_type == "send" && e.wave().is_some()).any(|e| {
                    e.str_field("copy")
                        .and_then(|c| std::path::Path::new(c).canonicalize().ok())
                        .zip(here.as_ref())
                        .is_some_and(|(copy, here)| here.starts_with(copy))
                }) {
                    return refuse(Refusal::WaveByBacklog);
                }
            }
    // A entrega e o veredito são as exceções dos tipos do binário: a onda e o
    // revisor gravam a própria volta, conferida mais abaixo, e a rodada ou o
    // fechamento grava a versão oficial.
    if let Err(refusal) = draft_rules(&opts.root, spec, event_type, &mut draft, true) {
        return refuse(refusal);
    }
    if let Err(refusal) = write::spec_was_opened(&project.root, spec) {
        return refuse(refusal);
    }
    // O pedido novo passa pela pergunta do lugar dele — desta spec, de uma
    // spec nova ou da lista de pendências do projeto — e, ao mesmo tempo, a
    // tarefa e o pedido novos passam pela comparação com as tarefas abertas.
    let route_question = if event_type == "request" {
        store::spec_file(&project.root, spec).and_then(|path| store::read(&path)).ok().flatten().and_then(|log| super::request_route::question(&log, &draft))
    } else {
        None
    };
    let same_work = same_work_question(&project.root, spec, event_type, &draft);
    let payloads: Vec<String> = route_question.iter().chain(same_work.as_ref().map(|same| &same.classifying.payload)).cloned().collect();
    // Com o recibo da reserva desta mesma decisão, ele responde no lugar do
    // Jev; sem o recibo ainda, a decisão segue pendente.
    let reserved = match reserve_id.as_deref().map(|id| reserve::reserved(&project.root, id, &payloads)) {
        Some(Reserved::Ready(docs)) => Some(docs),
        Some(Reserved::Pending) => {
            let pending = reserve::reserve_request(&project.root, spec, TASK_TRIAGE_CALL, &payloads);
            if pending.get("unavailable").is_none() {
                return Err(super::request_route::to_reserve(pending, event_type, lang));
            }
            None
        }
        Some(Reserved::Stale) | None => None,
    };
    let from_reserve = reserved.is_some();
    let mut answers = reserved.map_or_else(|| ask_together(&project.root, spec, &payloads, ask), |docs| docs.into_iter().map(Asked::Answered).collect()).into_iter();
    let route_answer = route_question.as_ref().and_then(|_| answers.next());
    let same_answer = same_work.as_ref().and_then(|_| answers.next());
    // O Jev ligado que falhou, passou do prazo ou voltou ilegível, numa
    // pergunta que ainda decide, manda a tarefa ou o pedido novo à reserva.
    let missed = |answer: &Asked<Value>, readable: &dyn Fn(&Value) -> bool| match answer {
        Asked::Failed => true,
        Asked::Answered(doc) => !readable(doc),
        Asked::Off => false,
    };
    let mut to_reserve = false;
    let mut route_warning = None;
    let confirm = || Some(translate("triage.request_unsure", lang).to_string());
    match &route_answer {
        Some(Asked::Answered(doc)) => match super::request_route::read(doc) {
            Some(Route::NewSpec) => return Err(super::request_route::new_spec(&draft, lang)),
            Some(Route::Pending) => match super::request_route::to_pending(&project.root, spec, &draft, lang) {
                Ok(moved) => return Err(moved),
                Err(_) => route_warning = confirm(),
            },
            Some(Route::Unsure) => route_warning = confirm(),
            Some(Route::ThisSpec) => {}
            None => to_reserve = true,
        },
        Some(Asked::Failed) => to_reserve = true,
        Some(Asked::Off) | None => {}
    }
    to_reserve |= same_answer.as_ref().is_some_and(|answer| missed(answer, &|doc| doc["answers"].is_object()));
    if to_reserve && !from_reserve {
        let request = reserve::reserve_request(&project.root, spec, TASK_TRIAGE_CALL, &payloads);
        if request.get("unavailable").is_none() {
            return Err(super::request_route::to_reserve(request, event_type, lang));
        }
        // Sem a reserva, a decisão do pedido fica em dúvida: grava, e quem
        // conduz confirma com o usuário; a tarefa grava como sempre.
        if event_type == "request" {
            route_warning = confirm();
        }
    }
    let same_doc = match (&same_work, same_answer) {
        (Some(_), Some(Asked::Answered(doc))) => Some(doc),
        _ => None,
    };
    if let (Some(same), Some(doc)) = (&same_work, same_doc)
        && let Some(mut refusal) = same_work_open(same, event_type, &draft, lang, &doc)
    {
        if let Some(warning) = route_warning {
            refusal["warnings"] = json!([warning]);
        }
        return Err(refusal);
    }
    // A skill criada ou atualizada ganha a impressão da versão que a onda
    // deixou no caminho reservado da cópia, que a rodada grava no lugar dela;
    // depois de gravada, sai do git e entra no resumo do projeto.
    let skill = if event_type == "skill" {
        if let Err(refusal) = crate::shared::failure_skills::prepare_skill(&opts.root, spec, &mut draft, lang) {
            return Err(refusal);
        }
        let field = |name: &str| draft.get(name).and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();
        Some((field("name"), field("action")))
    } else {
        None
    };
    // O passo de término de uma tarefa guarda a impressão da cópia da onda,
    // que a entrega compara com a cópia dela na hora de entregar.
    // O passo cujo item não é tarefa nem critério da onda grava, com aviso.
    let stray_step =
        if event_type == "step" { mark_step_copy(&project.root, spec, &mut draft, lang) } else { None };
    // A volta da onda só entra com envio aberto para ela, e passa antes pelas
    // conferências que só leem a volta: a recusa vem antes de gravar, e o
    // agente grava de novo. A trava do passo do git que a conferência prende
    // fica presa até a volta estar no arquivo: a rodada que assume a mesma
    // onda nunca grava a entrega oficial entre a conferência e a gravação. O
    // veredito do revisor prende a mesma trava, para o fechamento não assumir
    // o pedido de revisão entre a conferência dele e a gravação.
    let (held, proof_warnings) = if ["delivered", "verdict"].contains(&event_type) {
        match check_return(&opts.root, spec, event_type, &mut draft, lang) {
            Ok((held, warnings)) => (Some(held), warnings),
            Err(refusal) => return Err(refusal.to_value(lang)),
        }
    } else {
        (None, Vec::new())
    };
    // A volta do revisor só entra com pedido de revisão aberto, e o veredito
    // final que não responde por todo o combinado recusa antes de gravar.
    if event_type == "verdict"
        && let Err(refusal) = check_verdict_return(&opts.root, spec, &mut draft)
    {
        return Err(refusal.to_value(lang));
    }
    // O passo seguinte de um pedido, pelo efeito dele; a conferência do tipo
    // recusa um efeito que não existe antes de o relatório sair.
    let next = (event_type == "request").then(|| draft.get("effect").and_then(Value::as_str).map(|effect| format!("request.{}", effect.trim()))).flatten();
    let spec = spec.to_string();
    let event_type = event_type.to_string();
    Ok(Ready { project, spec, event_type, draft, held, warnings: stray_step.into_iter().chain(proof_warnings).chain(route_warning).collect(), skill, next })
}

/// Grava o rascunho pronto `ready`, visto de `start`, sob a trava da spec: a
/// remoção com a volta das tarefas da onda que ela tira, e o resto pela
/// gravação comum.
fn record_ready(ready: &mut Ready, start: &Path, session: Option<&str>) -> Result<Recorded, Refusal> {
    let draft = std::mem::take(&mut ready.draft);
    let (project, spec) = (&ready.project, ready.spec.as_str());
    if ready.event_type == "remove" {
        return record_removal(project, start, spec, draft, session);
    }
    write::record_in(project, start, spec, &ready.event_type, draft, None, session)
}

/// O relatório da gravação `recorded` do rascunho pronto `ready`, visto de
/// `start`, como o `run write` o imprime. Solta a trava do passo do git que a
/// volta prendia. Com `copy`, a resposta só avisa que a cópia virou
/// publicação explícita.
fn finish(ready: Ready, start: &Path, copy: bool, recorded: Result<Recorded, Refusal>) -> Value {
    let Ready { project, spec, event_type, held, warnings: early, skill, next, .. } = ready;
    drop(held);
    let (spec, event_type, lang) = (spec.as_str(), event_type.as_str(), project.lang);
    match recorded {
        Ok(Recorded { written, survey, returned, emptied_waves }) => {
            // O item gravado entra no bloco das specs do mapa. A gravação na
            // spec já está feita: a falha no mapa não a desfaz, e a próxima
            // resposta do mapa tenta de novo.
            let _ = mustard_core::io::map_specs::sync_spec(&project.root, spec, &project.languages);
            let mut report = json!({
                "ok": true,
                "spec": spec.trim(),
                "id": written.id,
                "type": event_type,
            });
            if let Some(code) = &written.code {
                report["code"] = json!(code);
            }
            if !written.removed.is_empty() {
                report["removed"] = json!(written.removed);
            }
            if !written.purged.is_empty() {
                report["purged"] = json!(written.purged);
            }
            if !returned.is_empty() {
                report["returned"] = json!(returned);
            }
            if !emptied_waves.is_empty() {
                report["waves_left_without_tasks"] = json!(emptied_waves);
            }
            // Se não deu para refazer a linha da spec no índice, o evento já
            // está no arquivo: fica o aviso. O nome citado num fato que o
            // mapa não confirma também só avisa.
            let mut warnings = Vec::new();
            if let Some(refusal) = &written.index_warning {
                warnings.push(spec_index::write_warning(refusal, lang));
            }
            for (fact, finding) in &written.citation_warnings {
                warnings.extend(finding.warning(*fact, lang));
            }
            warnings.extend(early);
            if let Some((name, action)) = &skill {
                warnings.extend(crate::shared::failure_skills::after_skill_written(start, spec, (written.id, name, action), lang));
            }
            // Só a gravação com `--copy` prepara a cópia: a última de um
            // pedido do usuário que muda o plano, que a leva uma vez, já com
            // tudo o que o pedido gerou. A ordem sai sem marco: a página que
            // ainda não tem endereço fica para o marco seguinte, e sem nada a
            // copiar a saída não traz cópia. A que não pôde ser preparada só
            // avisa, e a cópia seguinte leva os mesmos itens.
            let next = next.map(|key| translate(&key, lang).to_string());
            if copy {
                warnings.push(match lang {
                    Locale::PtBr => "`--copy` foi substituído por publicação explícita: use `/mustard:pages`. O estado local já está atualizado.".into(),
                    Locale::EnUs => "`--copy` is replaced by explicit publishing: use `/mustard:pages`. Local state is already current.".into(),
                });
            }
            if !warnings.is_empty() {
                report["warnings"] = json!(warnings);
            }
            if let Some(next) = next {
                report["next"] = json!(next);
            } else if let Some(survey) = survey {
                for (key, value) in survey {
                    report[key.as_str()] = value;
                }
            }
            report
        }
        Err(refusal) => super::refused(&refusal, lang),
    }
}

/// A pergunta do classificador das atividades sobre a tarefa ou o pedido
/// novo, com a spec lida em que ela se responde.
struct SameWork {
    root: std::path::PathBuf,
    log: SpecLog,
    classifying: Classifying,
}

/// A pergunta do classificador das atividades ([`task_triage::classifying`])
/// sobre a tarefa ou o pedido novo `draft`, com as tarefas do backlog sem onda
/// da spec `spec` de `root` ([`backlog_left`]). A tarefa de uma onda — a
/// reprovada, a em conserto, a que voltou — não entra: a que roda é assunto da
/// espera pela onda semelhante, e o conserto das outras é trabalho novo. A
/// versão nova (`replaces`), a gravação com o motivo em `distinct` e a spec
/// sem tarefa no backlog não perguntam: `None`.
fn same_work_question(root: &Path, spec: &str, event_type: &str, draft: &Map<String, Value>) -> Option<SameWork> {
    if !matches!(event_type, "task" | "request") || draft.contains_key("replaces") || draft.contains_key("distinct") {
        return None;
    }
    let log = store::read(&store::spec_file(root, spec).ok()?).ok()??;
    let open: Vec<&SpecEvent> = backlog_left(&log).into_iter().filter_map(|id| log.get(id)).collect();
    if open.is_empty() {
        return None;
    }
    let candidates: Vec<Candidate> = open
        .iter()
        .map(|task| Candidate {
            id: task.id,
            title: task.str_field("title").unwrap_or_default(),
            text: task.str_field("text").unwrap_or_default(),
            files: task_files(task),
            joinable: true,
        })
        .collect();
    let said = |name: &str| draft.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    let files = draft
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| file.as_str().or_else(|| file.get("path").and_then(Value::as_str)))
        .map(str::to_string)
        .collect();
    let item = Item { title: said("title"), text: said("text"), files };
    let classifying = task_triage::classifying(&candidates, &[item], &[])?;
    Some(SameWork { root: root.to_path_buf(), log, classifying })
}

/// A recusa da tarefa ou do pedido novo, do assistente ou do plano, que a
/// resposta `doc` do Jev à pergunta `same` ([`same_work_question`]) — ou da
/// reserva dele — dá como o mesmo trabalho de uma tarefa do backlog sem onda:
/// a coberta (terminar a tarefa termina a atividade) ou a de mesmo sentido,
/// que entra nela. A recusa (`same-work-open`) nomeia a tarefa, no idioma
/// `lang`, e manda gravar a versão nova dela; na junção, traz em `rewrite` o
/// pedido ao agente de análise da sessão, que escreve o texto da versão nova.
/// O trabalho novo e a leitura sem confiança gravam como sempre: `None`.
fn same_work_open(same: &SameWork, event_type: &str, draft: &Map<String, Value>, lang: Locale, doc: &Value) -> Option<Value> {
    let (task, key, joins) = match same.classifying.read(doc).into_iter().next().flatten()? {
        Destination::Covered { task, .. } => (task, "triage.same_work_covered", false),
        Destination::Join { task, .. } => (task, "triage.same_work_open", true),
        Destination::New { .. } => return None,
    };
    let event = same.log.get(task)?;
    let code = same.log.codes().get(&task).cloned().unwrap_or_else(|| task.to_string());
    let hint = translate(key, lang).replace("{task}", &code).replace("{title}", event.str_field("title").unwrap_or_default());
    let mut out = json!({ "ok": false, "reason": "same-work-open", "hint": hint, "task": code, "id": task });
    if joins {
        let sources = vec![task_triage::rewrite_source(code, &event.fields), task_triage::rewrite_source(event_type.to_string(), draft)];
        out["rewrite"] = task_triage::rewrite_request(&same.root, task, sources);
    }
    Some(out)
}

/// A remoção que o modelo grava e, no mesmo trecho com a trava do arquivo
/// presa, a volta ao backlog das tarefas da onda que ela tirou sem entrega
/// ([`SpecLog::tasks_of_removed_waves`]): cada uma ganha a versão sem `wave`
/// ([`backlog_return`]), gravada pelo programa. Sem isso a tarefa ficaria
/// presa ao número de uma onda que não existe mais, fora do backlog. A
/// remoção diz também as ondas com envio aberto que ela tirou do ar
/// ([`SpecLog::waves_left_without_tasks`]): as que ficaram sem tarefa por ela, e não
/// as que já estavam assim antes. Quem remove a tarefa errada vê que a onda
/// que queria tirar do ar não está na lista.
///
/// # Errors
///
/// A recusa da remoção, sem nada gravado, ou a da volta de uma tarefa.
fn record_removal(project: &super::Project, start: &Path, spec: &str, draft: Map<String, Value>, session: Option<&str>) -> Result<Recorded, Refusal> {
    let path = store::spec_file(&project.root, spec)?;
    let held = store::with_locked_writer(&path, |locked| removal_locked(locked, project, start, spec, draft.clone(), session))?;
    // Sem o arquivo de eventos, a gravação comum dá a recusa de sempre.
    held.unwrap_or_else(|| write::record_in(project, start, spec, "remove", draft, None, session))
}

/// [`record_removal`] dentro da trava da spec que `locked` já prende.
///
/// # Errors
///
/// A recusa da remoção ou a da volta de uma tarefa.
fn removal_locked(
    locked: &mut store::LockedLog,
    project: &super::Project,
    start: &Path,
    spec: &str,
    draft: Map<String, Value>,
    session: Option<&str>,
) -> Result<Recorded, Refusal> {
    let before = locked.log().waves_left_without_tasks();
    let mut removal = write::record_to(project, start, spec, "remove", draft, None, Some(&mut *locked), session)?;
    let freed: Vec<Map<String, Value>> = locked.log().tasks_of_removed_waves(&removal.written.removed).into_iter().map(backlog_return).collect();
    for task in freed {
        let back = write::record_to(project, start, spec, "task", task, Some(PhaseWriter::Binary), Some(&mut *locked), session)?;
        removal.returned.push(back.written.id);
    }
    removal.emptied_waves = locked.log().waves_left_without_tasks().into_keys().filter(|wave| !before.contains_key(wave)).collect();
    Ok(removal)
}

/// O aviso de que esta gravação veio de outra sessão: a spec já tinha
/// registro de uma sessão, e esta é outra. `None` quando a sessão é a mesma,
/// quando a spec não tem registro de sessão nenhuma, ou quando esta chamada
/// não sabe de que sessão veio: a sessão `mine` é a que a entrada passou.
///
/// Uma conversa paralela no mesmo projeto gravou as leituras dela dentro da
/// spec da conversa principal, e pelo registro não se sabia quem fez o quê. O
/// aviso é só aviso: duas sessões na mesma spec podem ser o que o usuário
/// quer, e travar a gravação travaria o condutor pelo motivo errado.
///
/// Só conta a conversa ligada a esta spec, a minha e a outra: a sessão que o
/// `/clear` encerrou, ou a que nunca abriu nem retomou a spec, não é outra
/// conversa na obra.
fn another_session_wrote(root: &Path, spec: Option<&str>, mine: Option<&str>, lang: Locale) -> Option<String> {
    let mine = mine.map(str::trim).filter(|mine| !mine.is_empty())?;
    let spec = spec.map(str::trim).filter(|spec| !spec.is_empty())?;
    let bound = |session: &str| crate::shared::context::session::spec_for_session(&root.to_string_lossy(), session).as_deref() == Some(spec);
    if !bound(mine) {
        return None;
    }
    let path = store::spec_file(root, spec).ok()?;
    let log = store::read(&path).ok().flatten()?;
    let theirs = another_session_in(&log, mine, bound)?;
    Some(crate::shared::say::say("spec_events.another_session", lang, &[("{session}", &theirs), ("{mine}", mine)]))
}

/// A sessão mais recente da spec lida em `log` que não é `mine` e que
/// `bound` diz ligada a ela; `None` quando não há outra assim.
fn another_session_in(log: &SpecLog, mine: &str, bound: impl Fn(&str) -> bool) -> Option<String> {
    log.events
        .iter()
        .rev()
        .filter_map(|event| event.str_field("session"))
        .find(|session| *session != mine && bound(session))
        .map(str::to_string)
}

/// O campo da resposta do passo com o bloco da próxima tarefa da onda.
pub(crate) const NEXT_TASK_FIELD: &str = "next_task";

/// O campo da resposta do passo que diz que todas as tarefas da onda
/// terminaram, com a ordem de entregar.
pub(crate) const TASKS_FINISHED_FIELD: &str = "tasks_finished";

/// Põe na resposta do passo aceito `report` o que vem depois dele
/// ([`after_step`]):
/// o bloco da próxima tarefa, já liberada, ou o aviso de que todas terminaram.
/// A spec é a do relatório, a do `--spec` ou a ativa da sessão `session` da
/// entrada, que a liberação também leva.
fn answer_next_task(report: &mut Value, project: &super::Project, named: Option<&str>, session: Option<&str>, json: &str, started: std::time::Instant) {
    let spec = report
        .get("spec")
        .and_then(Value::as_str)
        .or(named)
        .map(str::trim)
        .filter(|spec| !spec.is_empty())
        .map(str::to_string)
        .or_else(|| DiskSpecState::new(&checkout(&project.root)).active(session));
    let Some(spec) = spec else { return };
    match after_step(&project.root, &spec, session, json, started) {
        Some(Next::Task(text)) => report[NEXT_TASK_FIELD] = Value::String(text),
        Some(Next::Finished) => report[TASKS_FINISHED_FIELD] = Value::String(translate("conversation_size.tasks_finished", project.lang).to_string()),
        None => {}
    }
}

/// Run `write` and print the JSON report; with `copy`, the write also
/// prepares the copy of the spec page. Exit 1 on a refusal.
pub fn run(opts: &WriteOpts, copy: bool) {
    let started = std::time::Instant::now();
    // A entrada lê a sessão uma vez e a passa a tudo o que grava.
    let session = session_from_env();
    let (mut report, json) = write_for(opts, copy, session.as_deref());
    // A gravação que veio de outra sessão avisa: duas conversas no mesmo
    // projeto gravam na mesma spec, e quem conduz não via a gravação da
    // outra. O aviso não segura nada — a decisão é de quem lê.
    if report["ok"] == json!(true) {
        let project = super::project(&opts.root);
        let spec = opts.spec.as_deref().or_else(|| report["spec"].as_str()).map(str::to_string);
        if let Some(hint) = another_session_wrote(&project.root, spec.as_deref(), session.as_deref(), project.lang) {
            super::pages::push_warning(&mut report, "another-session", &hint);
        }
    }
    // O passo corrigido por um rascunho traz só o campo que faltava: a
    // resposta dele lê os campos como foram gravados, com o rascunho juntado.
    let written = WriteOpts { root: opts.root.clone(), spec: opts.spec.clone(), event_type: opts.event_type.clone(), json };
    answer_written_step(&mut report, &written, session.as_deref(), started);
    let _ = super::conversation::record_call_as(&opts.root, session.as_deref(), "write", opts.spec.as_deref(), started, &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

/// O passo `opts` gravado, com o relatório `report`, responde ele mesmo a
/// medida da conversa do agente de onda que o gravou, pela identidade que o
/// gancho do terminal exporta ao comando. Abaixo do limite, ou sem medida, a
/// resposta traz a próxima tarefa da onda, que fica liberada; acima, só a
/// ordem de entregar. Outro tipo, ou o passo recusado, fica como está.
fn answer_written_step(report: &mut Value, opts: &WriteOpts, session: Option<&str>, started: std::time::Instant) {
    if opts.event_type.trim() != "step" || report["ok"] != json!(true) {
        return;
    }
    let project = super::project(&opts.root);
    let transcript = session.and_then(|session| crate::shared::typed_messages::session_transcript(&project.root, session));
    let deliver = crate::hooks::session::conversation_size::answer_step(
        report,
        &project.root,
        &opts.event_type,
        &opts.json,
        crate::shared::search_gateway::agent_from_env().as_deref(),
        session,
        transcript.as_deref(),
        project.lang,
    );
    if deliver != Some(true) {
        answer_next_task(report, &project, opts.spec.as_deref(), session, &opts.json, started);
    }
}

/// Os campos da resposta de uma gravação que a linha curta do lote mantém:
/// a ordem ao agente que só a resposta traz — a próxima tarefa liberada, a de
/// entregar, o pedido que a recusa manda atender antes de gravar de novo e o
/// número do rascunho que a recusa guardou para a correção, com a dica dela.
const ORDER_FIELDS: &[&str] = &[NEXT_TASK_FIELD, TASKS_FINISHED_FIELD, "reserve", "rewrite", "request", "draft", "draft_hint"];

/// Uma linha do lote, lida da entrada.
struct BatchLine {
    /// O número da linha na entrada, contando as vazias.
    number: usize,
    /// O rótulo com que as linhas seguintes a citam.
    label: Option<String>,
    /// O tipo e os campos, ou a recusa da linha que não se lê.
    item: Result<(String, Map<String, Value>), Value>,
}

/// As linhas não vazias da entrada `input` do lote: cada uma é um objeto JSON
/// com o tipo em `type` e os outros campos do evento.
fn batch_lines(input: &str, lang: Locale) -> Vec<BatchLine> {
    input
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(at, line)| {
            let item = write::draft_object(line).map_err(|refusal| super::refused(&refusal, lang)).and_then(|mut fields| {
                match fields.remove("type").as_ref().and_then(Value::as_str).map(str::trim).filter(|kind| !kind.is_empty()) {
                    Some(kind) => Ok((kind.to_string(), fields)),
                    None => Err(json!({ "ok": false, "reason": "batch-type-missing", "hint": translate("spec_events.batch_type_missing", lang) })),
                }
            });
            let label = item.as_ref().ok().and_then(|(_, fields)| fields.get("label")).and_then(Value::as_str).map(str::trim).filter(|label| !label.is_empty()).map(str::to_string);
            BatchLine { number: at + 1, label, item }
        })
        .collect()
}

/// A gravação em lote: cada linha de `input`, na ordem, passa pelas mesmas
/// conferências da gravação sozinha, na spec `spec` de `root`, e grava como
/// ela, sob a trava da spec, antes de a linha seguinte se conferir: a
/// conferência de cada linha — a do trabalho repetido, a da entrega — enxerga
/// as que o lote já gravou, e os rótulos que ela cita já chegam a ela como os
/// números dessas linhas. A linha recusada não grava, e a que cita pelo rótulo
/// uma linha do lote que não gravou também é recusada, apontando a linha de
/// origem. Devolve, na ordem, o número de cada linha e a resposta que a
/// gravação sozinha daria.
fn batch_for(root: &Path, spec: Option<&str>, input: &str, session: Option<&str>, ask: &AskJev<'_>) -> Vec<(usize, Value)> {
    let lang = super::project(root).lang;
    let lines = batch_lines(input, lang);
    let mut answers: Vec<Value> = Vec::with_capacity(lines.len());
    let mut ids: Vec<Option<u64>> = vec![None; lines.len()];
    // Os campos de cada linha como ela foi gravada, com o rascunho juntado:
    // é deles que a resposta do passo lê a onda e o item.
    let mut written: Vec<Option<String>> = vec![None; lines.len()];
    for at in 0..lines.len() {
        let (event_type, mut fields) = match &lines[at].item {
            Ok(item) => item.clone(),
            Err(refusal) => {
                answers.push(refusal.clone());
                continue;
            }
        };
        if let Some(refusal) = cites_unrecorded(&lines, &ids, at, &event_type, &fields, lang) {
            answers.push(refusal);
            continue;
        }
        let by_label = |label: &str| (0..at).rev().find(|before| lines[*before].label.as_deref() == Some(label)).and_then(|before| ids[before]);
        write::resolve_labels(&event_type, &mut fields, &by_label);
        // A linha que cita um rascunho de recusa parte dele; a recusa dela o
        // guarda de novo, e a que grava o tira.
        let draft = match Resolved::of(root, spec, session, &event_type, &Value::Object(fields).to_string()) {
            Ok(draft) => draft,
            Err(refusal) => {
                answers.push(refusal);
                continue;
            }
        };
        let opts = WriteOpts { root: root.to_path_buf(), spec: spec.map(str::to_string), event_type, json: draft.json().to_string() };
        written[at] = Some(opts.json.clone());
        let answer = match prepare(&opts, session, ask) {
            Err(answer) => answer,
            Ok(mut ready) => {
                let recorded = record_ready(&mut ready, root, session);
                ids[at] = recorded.as_ref().ok().map(|done| done.written.id);
                finish(ready, root, false, recorded)
            }
        };
        answers.push(draft.settle(answer));
    }
    lines
        .iter()
        .zip(answers)
        .zip(written)
        .map(|((line, mut answer), json)| {
            if let (Ok((event_type, _)), Some(json)) = (&line.item, json) {
                let opts = WriteOpts { root: root.to_path_buf(), spec: spec.map(str::to_string), event_type: event_type.clone(), json };
                answer_written_step(&mut answer, &opts, session, std::time::Instant::now());
            }
            (line.number, answer)
        })
        .collect()
}

/// A recusa da linha `at` do lote que cita pelo rótulo uma linha anterior que
/// não gravou, `ids` sem o número dela: a recusada, ou a que gravou fora da
/// spec. Cada rótulo aponta a linha mais próxima antes dela com ele.
fn cites_unrecorded(lines: &[BatchLine], ids: &[Option<u64>], at: usize, event_type: &str, fields: &Map<String, Value>, lang: Locale) -> Option<Value> {
    write::cited_labels(event_type, fields).into_iter().find_map(|label| {
        let origin = (0..at).rev().find(|before| lines[*before].label.as_deref() == Some(label.as_str()))?;
        ids[origin].is_none().then(|| {
            let hint = translate("spec_events.batch_line_refused", lang).replace("{line}", &lines[origin].number.to_string()).replace("{label}", &label);
            json!({ "ok": false, "reason": "batch-line-refused", "hint": hint, "line": lines[origin].number })
        })
    })
}

/// A linha curta do lote para a linha `number` com a resposta `answer`: o
/// código gravado, ou o número do evento sem código; ou a razão e a dica da
/// recusa. Os avisos da gravação — a citação que o mapa não confirma, a
/// prova, o passo fora da onda, a dúvida do lugar do pedido — e os campos de
/// ordem ao agente ([`ORDER_FIELDS`]) seguem junto.
fn batch_line(number: usize, answer: &Value) -> Value {
    let mut line = json!({ "line": number });
    let kept: &[&str] = if answer["ok"] == json!(true) {
        if answer.get("code").is_some() { &["code", "warnings"] } else { &["id", "warnings"] }
    } else {
        &["reason", "hint", "warnings"]
    };
    for field in kept.iter().chain(ORDER_FIELDS) {
        if let Some(value) = answer.get(*field) {
            line[*field] = value.clone();
        }
    }
    line
}

/// As linhas curtas da gravação em lote de `input` ([`batch_for`],
/// [`batch_line`]), na ordem, pela sessão `session` da entrada. Como na
/// gravação sozinha, a primeira linha gravada em cada spec avisa quando outra
/// sessão gravou nela ([`another_session_wrote`]).
fn batch_report(root: &Path, spec: Option<&str>, input: &str, session: Option<&str>, ask: &AskJev<'_>) -> Vec<Value> {
    let project = super::project(root);
    let mut seen: Vec<String> = Vec::new();
    batch_for(root, spec, input, session, ask)
        .iter()
        .map(|(number, answer)| {
            let mut line = batch_line(*number, answer);
            let written = spec.or_else(|| answer["spec"].as_str()).filter(|_| answer["ok"] == json!(true)).map(str::trim);
            if let Some(written) = written.filter(|written| !seen.iter().any(|had| had == written)) {
                if let Some(hint) = another_session_wrote(&project.root, Some(written), session, project.lang) {
                    super::pages::push_warning(&mut line, "another-session", &hint);
                }
                seen.push(written.to_string());
            }
            line
        })
        .collect()
}

/// Run `write --batch`: read the lines of standard input, write them as
/// [`batch_report`] does and print one short line per item, in order. Exit 1
/// when a line was refused.
pub fn run_batch(root: &Path, spec: Option<&str>) {
    let started = std::time::Instant::now();
    let session = session_from_env();
    let lang = super::project(root).lang;
    let ask = |root: &Path, spec: &str, payload: &str| reserve::ask_orchestration(root, spec, TASK_TRIAGE_CALL, payload);
    let lines: Vec<Value> = match std::io::read_to_string(std::io::stdin()) {
        Ok(input) if input.trim().is_empty() => vec![json!({ "ok": false, "reason": "batch-empty", "hint": translate("spec_events.batch_empty", lang) })],
        Ok(input) => batch_report(root, spec, &input, session.as_deref(), &ask),
        Err(error) => vec![write::RawRefusal::Stdin { detail: error.to_string() }.to_value(lang)],
    };
    let refused = lines.iter().find(|line| line.get("reason").is_some());
    let summary = json!({ "ok": refused.is_none(), "reason": refused.map(|line| line["reason"].clone()), "lines": lines.len() });
    let output: String = lines.iter().map(|line| format!("{line}\n")).collect();
    let _ = super::conversation::record_output_call_as(root, session.as_deref(), "write", spec, started, &summary, &output);
    print!("{output}");
    if refused.is_some() {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::spec_events::parse_log;
    use tempfile::tempdir;

    /// Grava na spec `teste`, aberta antes como o comando que abre uma spec a
    /// deixa, como a produção grava ([`write::seed_at`]).
    fn write(root: &Path, event_type: &str, json: &str) -> Value {
        let path = store::spec_file(&store::spec_root(root), "teste").unwrap();
        if !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap()).expect("spec folder");
            std::fs::File::create(&path).expect("the event file");
        }
        write::seed_at(&WriteOpts { root: root.to_path_buf(), spec: Some("teste".into()), event_type: event_type.into(), json: json.into() })
    }

    fn message(root: &Path, author: &str, text: &str) -> u64 {
        write(root, "message", &json!({ "author": author, "text": text }).to_string())["id"].as_u64().unwrap()
    }

    /// Um `context` cujo `title` é o próprio texto.
    fn context(root: &Path, text: &str, origin: u64) -> Value {
        write(root, "context", &json!({ "title": text, "text": text, "origin": origin }).to_string())
    }

    /// A spec com registro de outra sessão ligada a ela acusa qual é ela, e
    /// a sessão mais recente que não é a minha é a que o aviso nomeia. Com só
    /// a minha, sem sessão nenhuma, ou com a outra já sem ligação, nada se diz.
    #[test]
    fn the_write_sees_the_other_session_in_the_spec() {
        let line = |id: u64, session: &str| {
            format!("{{\"v\":1,\"id\":{id},\"at\":\"2026-10-22T10:0{id}:00Z\",\"type\":\"call\",\"author\":\"binary\",\"command\":\"read\",\"ms\":1,\"result\":\"ok\",\"session\":\"{session}\"}}")
        };
        let all = |_: &str| true;
        let log = parse_log(&format!("{}\n{}\n{}\n", line(1, "a"), line(2, "b"), line(3, "a")));
        assert_eq!(another_session_in(&log, "a", all).as_deref(), Some("b"), "a mais recente que não é a minha");
        assert_eq!(another_session_in(&log, "b", all).as_deref(), Some("a"));
        assert_eq!(another_session_in(&log, "a", |session| session != "b"), None, "a outra sem ligação não conta");
        let only_mine = parse_log(&format!("{}\n", line(1, "a")));
        assert_eq!(another_session_in(&only_mine, "a", all), None, "só a minha não é outra");
        assert_eq!(another_session_in(&parse_log(""), "a", all), None, "sem registro, nada a dizer");
    }

    /// O item gravado pela porta do `run write` leva a sessão que a entrada
    /// passou, e nenhuma quando ela não passou: a do ambiente de quem roda
    /// não entra.
    #[test]
    fn the_written_item_carries_the_session_the_entry_gave() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        write::record_open(root, "marca", "feature/marca", "dev").unwrap();
        super::super::session_spec::bind(root, Some("s-recebida"), "marca");
        let asked = super::super::conversation::record_message(root, Some("s-recebida"), "Anote o pedido.").unwrap();
        let said = |text: &str, session: Option<&str>| {
            let json = json!({ "title": "Anotar o pedido", "text": text, "keys": ["pedido"], "origin": asked }).to_string();
            let opts = WriteOpts { root: root.to_path_buf(), spec: Some("marca".into()), event_type: "note".into(), json };
            let (out, _) = write_for(&opts, false, session);
            assert_eq!(out["ok"], json!(true), "{out}");
            let log = store::read(&store::spec_file(root, "marca").unwrap()).unwrap().unwrap();
            log.get(out["id"].as_u64().unwrap()).and_then(|event| event.str_field("session")).map(str::to_string)
        };
        assert_eq!(said("com a sessão da entrada", Some("s-recebida")).as_deref(), Some("s-recebida"));
        assert_eq!(said("sem sessão nenhuma", None), None);
    }

    /// O lote gravado numa spec em que outra sessão ligada a ela já gravou
    /// avisa como a gravação sozinha: a primeira linha gravada leva o aviso
    /// com a sessão da outra, e a seguinte, na mesma spec, não o repete.
    #[test]
    fn a_batch_after_another_session_wrote_warns_like_a_single_write() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        write::record_open(root, "marca", "feature/marca", "dev").unwrap();
        super::super::session_spec::bind(root, Some("s-outra"), "marca");
        super::super::session_spec::bind(root, Some("s-minha"), "marca");
        let asked = super::super::conversation::record_message(root, Some("s-minha"), "Anote o pedido.").unwrap();
        let note = |title: &str| json!({ "title": title, "text": title, "keys": ["pedido"], "origin": asked });
        let theirs = WriteOpts { root: root.to_path_buf(), spec: Some("marca".into()), event_type: "note".into(), json: note("da outra").to_string() };
        assert_eq!(write_for(&theirs, false, Some("s-outra")).0["ok"], json!(true));

        let line = |title: &str| {
            let mut line = note(title);
            line["type"] = json!("note");
            line
        };
        let input = format!("{}\n{}\n", line("primeira"), line("segunda"));
        let lines = batch_report(root, Some("marca"), &input, Some("s-minha"), &|_, _, _| Asked::Off);
        assert!(lines.iter().all(|line| line.get("code").is_some()), "{lines:?}");
        let warnings = |line: &Value| line["warnings"].as_array().cloned().unwrap_or_default();
        let first = warnings(&lines[0]);
        assert_eq!(first.len(), 1, "{lines:?}");
        assert_eq!(first[0]["reason"], json!("another-session"));
        assert!(first[0]["hint"].as_str().unwrap_or_default().contains("s-outra"), "{first:?}");
        assert!(warnings(&lines[1]).is_empty(), "one warning per spec: {lines:?}");
    }

    /// Cada linha do lote se confere depois de as anteriores gravarem: a
    /// tarefa repetida na linha seguinte vai ao Jev com a primeira entre as
    /// abertas e volta recusada, nomeando a primeira, como na gravação
    /// sozinha. Só a primeira grava.
    #[test]
    fn a_batch_line_is_checked_after_the_lines_before_it_are_written() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let asked = write(root, "message", r#"{"author":"user","text":"inclua a subtração"}"#)["id"].as_u64().unwrap();
        let request = json!({"title": "Subtração", "text": "Incluir a subtração.", "keys": ["subtração"], "effect": "new_waves",
            "agent": "- conferir pelo teste. Prova: `git --version`", "origin": asked});
        let request = write(root, "request", &request.to_string())["id"].as_u64().unwrap();
        let task = json!({"type": "task", "title": "Subtrair", "text": "Subtrair dois números.", "agent": "- subtrair",
            "files": [{"path": "src/calc.ts"}], "depends_on": [], "covers": [request], "origin": asked});
        let ask = |root: &Path, spec: &str, _: &str| {
            let log = store::read(&store::spec_file(root, spec).unwrap()).unwrap().unwrap();
            let open = backlog_left(&log).into_iter().next().expect("the task of the first line is open");
            Asked::Answered(triaged(&format!("join:t{open}")))
        };

        let answers = batch_for(root, Some("teste"), &format!("{task}\n{task}\n"), None, &ask);
        let first = answers[0].1["code"].as_str().unwrap_or_else(|| panic!("the first line is written: {answers:?}")).to_string();
        assert_eq!(answers[1].1["reason"], json!("same-work-open"), "{answers:?}");
        assert_eq!(answers[1].1["task"], json!(first), "the refusal names the first line: {answers:?}");
        let log = store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "task").count(), 1, "only the first line is written");
    }

    /// A spec com o pedido da subtração e uma tarefa aberta que o cobre, em
    /// `src/calc.ts`. Devolve a mensagem do usuário, o pedido e a tarefa.
    fn subtraction_backlog(root: &std::path::Path) -> (u64, u64, u64) {
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let asked = write(root, "message", r#"{"author":"user","text":"inclua a subtração"}"#)["id"].as_u64().unwrap();
        let request = json!({"title": "Subtração", "text": "Incluir a subtração.", "keys": ["subtração"], "effect": "new_waves",
            "agent": "- conferir pelo teste. Prova: `git --version`", "origin": asked});
        let request = write(root, "request", &request.to_string())["id"].as_u64().unwrap();
        let task = json!({"title": "Subtrair", "text": "Subtrair dois números.", "agent": "- subtrair", "files": [{"path": "src/calc.ts"}],
            "depends_on": [], "covers": [request], "origin": asked});
        let task = write(root, "task", &task.to_string());
        assert_eq!(task["ok"], json!(true), "{task}");
        (asked, request, task["id"].as_u64().unwrap())
    }

    /// A leitura confiante do Jev, com a escolha `choice`, para a pergunta
    /// da atividade gravada.
    fn triaged(choice: &str) -> Value {
        json!({"answers": {"i0": {"choice": choice, "confidence": 0.95, "probabilities": {choice: 0.93, "new": 0.04}}}})
    }

    /// A tarefa nova que o Jev, confiante, dá como o mesmo sentido da aberta
    /// é recusada nomeando a aberta, e nada se grava; com `distinct`, ela grava
    /// sem perguntar e guarda o motivo; sem Jev, grava como sempre.
    #[test]
    fn backlog_triage_refuses_a_task_with_the_meaning_of_an_open_one() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (asked, request, open) = subtraction_backlog(root);
        let again = json!({"title": "Tirar um número do outro", "text": "Tirar um número do outro.", "agent": "- tirar",
            "files": [{"path": "src/calc.ts"}], "depends_on": [], "covers": [request], "origin": asked});
        let opts = |draft: &Value| WriteOpts { root: root.to_path_buf(), spec: Some("teste".into()), event_type: "task".into(), json: draft.to_string() };
        let events = || store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap().events.len();
        let before = events();

        let refused = write_triaged(&opts(&again), false, None, &|_, _, _| Asked::Answered(triaged(&format!("join:t{open}"))));
        assert_eq!(refused["reason"], json!("same-work-open"), "{refused}");
        assert_eq!(refused["task"], json!("MSTD-TASK-0001"), "{refused}");
        assert!(refused["hint"].as_str().unwrap().contains("MSTD-TASK-0001 — Subtrair"), "{refused}");
        assert_eq!(events(), before, "nothing is recorded");

        let mut distinct = again.clone();
        distinct["distinct"] = json!("Outra conta: tira um número do outro.");
        let recorded = write_triaged(&opts(&distinct), false, None, &|_, _, _| panic!("a distinct write asks nothing"));
        assert_eq!(recorded["ok"], json!(true), "{recorded}");
        let log = store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap();
        let id = recorded["id"].as_u64().unwrap();
        assert_eq!(log.get(id).and_then(|event| event.str_field("distinct")), Some("Outra conta: tira um número do outro."));

        let plain = write_triaged(&opts(&again), false, None, &|_, _, _| Asked::Off);
        assert_eq!(plain["ok"], json!(true), "without Jev the task is recorded: {plain}");
    }

    /// Na junção, a recusa traz o pedido ao agente de análise da sessão, no
    /// modelo pequeno da família do anfitrião, com a tarefa e a atividade
    /// como fontes; o recibo devolve o título, o texto e a parte do agente
    /// juntos, e a versão nova que quem conduz grava com eles substitui a
    /// tarefa.
    #[test]
    fn backlog_triage_join_hands_the_text_to_the_session_small_model() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (asked, request, open) = subtraction_backlog(root);
        let again = json!({"title": "Tirar um número do outro", "text": "Tirar um número do outro.", "agent": "- tirar",
            "files": [{"path": "src/calc.ts"}], "depends_on": [], "covers": [request], "origin": asked});
        let opts = WriteOpts { root: root.to_path_buf(), spec: Some("teste".into()), event_type: "task".into(), json: again.to_string() };
        let refused = write_triaged(&opts, false, None, &|_, _, _| Asked::Answered(triaged(&format!("join:t{open}"))));
        let delegation = &refused["rewrite"]["delegation"];
        let family = if delegation["host"] == json!("claude") { "haiku" } else { "luna" };
        assert_eq!(delegation["model_family"], json!(family), "{refused}");
        assert_eq!(delegation["preferred_agent"], json!("mustard-analysis"), "{refused}");
        let prompt = delegation["request"]["prompt"].as_str().unwrap();
        assert!(prompt.contains("Subtrair dois números.") && prompt.contains("Tirar um número do outro."), "{prompt}");

        let receipt = mustard_core::io::knowledge::enrichment::SessionReceipt {
            request_id: refused["rewrite"]["request_id"].as_str().unwrap().to_string(),
            model: "inherit".into(),
            effort: None,
            result: json!({"title": "Subtrair de dois jeitos", "text": "Subtrair dois números e tirar um do outro.", "agent": "- subtrair\n- tirar"}),
            input_tokens: None,
            output_tokens: None,
        };
        let done = mustard_core::io::knowledge::enrichment::complete_session(root, root, &receipt).unwrap();
        let rewrite = &done["rewrite"];
        assert_eq!(rewrite["replaces"], json!(open), "{done}");
        let version = json!({"replaces": rewrite["replaces"], "title": rewrite["title"], "text": rewrite["text"], "agent": rewrite["agent"],
            "files": [{"path": "src/calc.ts"}], "depends_on": [], "covers": [request], "origin": asked});
        let recorded = write(root, "task", &version.to_string());
        assert_eq!(recorded["ok"], json!(true), "{recorded}");
        let log = store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap();
        assert_eq!(log.current(open).and_then(|task| task.str_field("title")), Some("Subtrair de dois jeitos"));
    }

    /// A tarefa nova que o Jev ligado não classifica não grava: a resposta
    /// traz o pedido de reserva com a mesma pergunta, e gravar de novo sem o
    /// recibo só a devolve. O recibo certo da junção recusa como o Jev; o
    /// incerto vale como dúvida e grava. Sem Jev, grava sem reserva.
    #[test]
    fn a_new_task_the_jev_misses_is_not_recorded_and_goes_to_the_reserve() {
        for certainty in ["sure", "unsure"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let (asked, request, open) = subtraction_backlog(root);
            let again = json!({"title": "Tirar um número do outro", "text": "Tirar um número do outro.", "agent": "- tirar",
                "files": [{"path": "src/calc.ts"}], "depends_on": [], "covers": [request], "origin": asked});
            let opts = |extra: &Value| {
                let mut draft = again.clone();
                for (key, value) in extra.as_object().into_iter().flatten() {
                    draft[key] = value.clone();
                }
                WriteOpts { root: root.to_path_buf(), spec: Some("teste".into()), event_type: "task".into(), json: draft.to_string() }
            };
            let tasks = || store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap().visible().iter().filter(|e| e.event_type == "task").count();
            let before = tasks();
            let out = write_triaged(&opts(&json!({})), false, None, &|_, _, _| Asked::Failed);
            assert_eq!(out["reason"], json!("jev-reserve"), "{out}");
            assert_eq!(tasks(), before, "nothing is recorded yet");
            let prompt = out["reserve"]["delegation"]["request"]["prompt"].as_str().unwrap();
            assert!(prompt.contains("Where does `items.i0` belong?"), "{prompt}");
            let id = out["reserve"]["request_id"].as_str().unwrap().to_string();
            let pending = write_triaged(&opts(&json!({ "reserve": id })), false, None, &|_, _, _| panic!("a pending reserve asks no Jev"));
            assert_eq!(pending["reason"], json!("jev-reserve"), "{pending}");

            let said = |choice: &str| json!({ "choice": choice, "certainty": certainty });
            let receipt = mustard_core::io::knowledge::enrichment::SessionReceipt {
                request_id: id.clone(),
                model: "inherit".into(),
                effort: None,
                result: json!({ "answers": { "q1": said(&format!("join:t{open}")), "q2": said("none") } }),
                input_tokens: None,
                output_tokens: None,
            };
            mustard_core::io::knowledge::enrichment::complete_session(root, root, &receipt).unwrap();
            let applied = write_triaged(&opts(&json!({ "reserve": id })), false, None, &|_, _, _| panic!("the receipt answers in place of the Jev"));
            if certainty == "sure" {
                assert_eq!(applied["reason"], json!("same-work-open"), "{applied}");
                assert_eq!(tasks(), before);
            } else {
                assert_eq!(applied["ok"], json!(true), "an unsure receipt is a doubt: {applied}");
                assert_eq!(tasks(), before + 1);
                let log = store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap();
                assert!(log.visible().iter().all(|event| !event.fields.contains_key("reserve")), "the reserve number stays out of the event");
            }
        }
    }

    /// A spec com o objetivo da calculadora e a mensagem do usuário que pede
    /// `asked`; devolve as opções da gravação do pedido com esse texto.
    fn routed_request(root: &std::path::Path, asked: &str) -> impl Fn(&Value) -> WriteOpts {
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let goal = message(root, "user", "Somar dois números na calculadora.");
        assert_eq!(context(root, "Somar dois números na calculadora.", goal)["ok"], json!(true));
        let said = message(root, "user", asked);
        let base = json!({"title": "Pedido do usuário", "text": asked, "keys": ["pedido"], "effect": "new_waves",
            "agent": "- conferir pelo teste", "origin": said});
        let root = root.to_path_buf();
        move |extra: &Value| {
            let mut draft = base.clone();
            for (key, value) in extra.as_object().into_iter().flatten() {
                draft[key] = value.clone();
            }
            WriteOpts { root: root.clone(), spec: Some("teste".into()), event_type: "request".into(), json: draft.to_string() }
        }
    }

    /// A resposta do Jev à pergunta do pedido: a escolha `choice`, com a
    /// confiança `confidence`, e a chance `fits` de o pedido ser do objetivo.
    fn route_answer(choice: &str, confidence: f64, fits: f64) -> Value {
        json!({"answers": {
            "fits": {"type": "noul", "noul": fits},
            "route": {"type": "choice", "choice": choice, "confidence": confidence, "probabilities": {choice: confidence}},
        }})
    }

    /// Só a pergunta do pedido ganha `answer`; a das tarefas abertas não tem
    /// resposta. Cada pergunta feita fica em `asked`.
    fn route_jev(answer: Value, asked: &std::sync::Mutex<Vec<String>>) -> impl Fn(&Path, &str, &str) -> Asked<Value> + Sync + '_ {
        move |_: &Path, _: &str, payload: &str| {
            asked.lock().unwrap().push(payload.to_string());
            if payload.contains("request-route") { Asked::Answered(answer.clone()) } else { Asked::Off }
        }
    }

    fn requests(root: &std::path::Path) -> usize {
        let log = store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap();
        log.visible().into_iter().filter(|event| event.event_type == "request").count()
    }

    /// O Jev certo de que o pedido é obra de outra spec: nada se grava, a
    /// recusa manda abrir uma spec nova e devolve o texto do pedido. A
    /// pergunta sai sem segredo. Com `same_spec`, a palavra do usuário de que
    /// o pedido é desta spec, a gravação passa sem perguntar.
    #[test]
    fn a_request_jev_reads_as_a_new_spec_is_refused_with_its_text() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let key = format!("ghp_{}", "a1".repeat(18));
        let asked_text = format!("Trocar o tema do site para escuro; o token {key} está no painel.");
        let opts = routed_request(root, &asked_text);
        let asked = std::sync::Mutex::new(Vec::new());
        let refused = write_triaged(&opts(&json!({})), false, None, &route_jev(route_answer("new_spec", 0.93, 0.04), &asked));
        assert_eq!(refused["ok"], json!(false), "{refused}");
        assert_eq!(refused["reason"], json!("request-new-spec"), "{refused}");
        assert!(refused["hint"].as_str().unwrap().contains("spec nova"), "{refused}");
        assert_eq!(refused["request"]["text"], json!(asked_text), "the request goes back: {refused}");
        assert_eq!(requests(root), 0, "nothing is recorded");
        let payloads = asked.lock().unwrap();
        assert_eq!(payloads.len(), 1, "one call: {payloads:?}");
        assert!(payloads[0].contains("Somar dois números") && payloads[0].contains("Trocar o tema"), "{}", payloads[0]);
        assert!(!payloads[0].contains(&key), "no secret leaves: {}", payloads[0]);
        drop(payloads);

        let confirmed = write_triaged(&opts(&json!({"same_spec": "O usuário disse que o tema é desta obra."})), false, None, &|_, _, payload| {
            assert!(!payload.contains("request-route"), "a confirmed request asks nothing");
            Asked::Off
        });
        assert_eq!(confirmed["ok"], json!(true), "{confirmed}");
        assert_eq!(requests(root), 1);
    }

    /// O Jev certo de que o pedido fica para depois: ele vai à lista de
    /// pendências do projeto, e não à spec, e a resposta diz onde está.
    #[test]
    fn a_request_jev_reads_as_for_later_goes_to_the_project_pending_list() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let opts = routed_request(root, "Mais para frente, revisar a política de backup do servidor.");
        let asked = std::sync::Mutex::new(Vec::new());
        let out = write_triaged(&opts(&json!({})), false, None, &route_jev(route_answer("project_pending", 0.9, 0.1), &asked));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["pending"], json!("P-1"), "{out}");
        assert!(out["hint"].as_str().unwrap().contains("P-1"), "{out}");
        assert_eq!(requests(root), 0, "the spec does not get it");
        let ledger = std::fs::read_to_string(root.join(".claude/pending/ledger.json")).unwrap();
        assert!(ledger.contains("revisar a política de backup"), "{ledger}");
    }

    /// O Jev certo de que o pedido é desta spec, e coerente na pergunta do
    /// objetivo: a gravação segue como sempre, sem aviso.
    #[test]
    fn a_request_jev_reads_as_this_spec_is_recorded_as_before() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let opts = routed_request(root, "Somar também números negativos.");
        let asked = std::sync::Mutex::new(Vec::new());
        let out = write_triaged(&opts(&json!({})), false, None, &route_jev(route_answer("same_spec", 0.95, 0.97), &asked));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(out.get("warnings").is_none(), "{out}");
        assert_eq!(requests(root), 1);
        assert_eq!(asked.lock().unwrap().len(), 1);
    }

    /// O Jev em dúvida — a escolha fraca, ou a escolha forte que a pergunta
    /// do objetivo desmente — grava como sempre, com o aviso de confirmar com
    /// o usuário.
    #[test]
    fn a_request_jev_doubts_is_recorded_with_a_warning_to_confirm() {
        for answer in [route_answer("new_spec", 0.55, 0.1), route_answer("new_spec", 0.95, 0.9), route_answer("same_spec", 0.95, 0.5)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let opts = routed_request(root, "Trocar o tema do site para escuro.");
            let asked = std::sync::Mutex::new(Vec::new());
            let out = write_triaged(&opts(&json!({})), false, None, &route_jev(answer.clone(), &asked));
            assert_eq!(out["ok"], json!(true), "{answer}: {out}");
            assert_eq!(requests(root), 1);
            let warnings = out["warnings"].to_string();
            assert!(warnings.contains("confirme com o usuário"), "{answer}: {out}");
        }
    }

    /// A linha do lote leva os avisos da gravação dela, como a gravação
    /// sozinha os devolve: o pedido cujo lugar o Jev põe em dúvida grava, e a
    /// linha curta traz o código e o aviso de confirmar com o usuário.
    #[test]
    fn a_batch_line_keeps_the_warnings_of_its_write() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let opts = routed_request(root, "Trocar o tema do site para escuro.");
        let mut line: Value = serde_json::from_str(&opts(&json!({})).json).unwrap();
        line["type"] = json!("request");
        let asked = std::sync::Mutex::new(Vec::new());
        let lines = batch_report(root, Some("teste"), &format!("{line}\n"), None, &route_jev(route_answer("new_spec", 0.55, 0.1), &asked));
        assert!(lines[0]["code"].is_string(), "the line is written: {lines:?}");
        assert!(lines[0]["warnings"].to_string().contains("confirme com o usuário"), "{lines:?}");
    }

    /// As duas perguntas do pedido novo ao Jev — o lugar dele e a comparação
    /// com as tarefas abertas — saem ao mesmo tempo: o Jev só responde cada
    /// uma quando as duas estão no ar, e uma depois da outra a primeira nunca
    /// seria respondida.
    #[test]
    fn a_new_request_asks_both_jev_questions_at_the_same_time() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let opts = routed_request(root, "Somar também números negativos.");
        subtraction_backlog(root);
        let before = requests(root);
        let asked = std::sync::Mutex::new(Vec::new());
        let both_in_flight = std::sync::Barrier::new(2);
        let out = write_triaged(&opts(&json!({})), false, None, &|_, _, payload| {
            both_in_flight.wait();
            asked.lock().unwrap().push(payload.to_string());
            Asked::Answered(if payload.contains("request-route") { route_answer("same_spec", 0.95, 0.97) } else { triaged("new") })
        });
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(requests(root), before + 1);
        assert_eq!(asked.lock().unwrap().len(), 2, "both questions went out");
    }

    /// O Jev que não responde a tempo (o filtro desiste em 10 s): o pedido
    /// não grava, e a resposta traz o pedido de reserva ao agente de análise
    /// com as duas perguntas. Sem o recibo, gravar de novo com a reserva só a
    /// devolve, pendente. Com o recibo certo de que o pedido fica para depois,
    /// a gravação de novo vai à lista de pendências do projeto, sem perguntar
    /// ao Jev; com o recibo incerto, grava na spec e manda confirmar.
    #[test]
    fn a_new_request_the_jev_misses_is_not_recorded_and_goes_to_the_reserve() {
        for (certainty, place) in [("sure", "pending"), ("unsure", "spec")] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let opts = routed_request(root, "Mais para frente, revisar a política de backup do servidor.");
            subtraction_backlog(root);
            let before = requests(root);
            let out = write_triaged(&opts(&json!({})), false, None, &|_, _, _| Asked::Failed);
            assert_eq!(out["ok"], json!(false), "{out}");
            assert_eq!(out["reason"], json!("jev-reserve"), "{out}");
            assert!(out["hint"].as_str().unwrap().contains("`reserve`"), "{out}");
            assert_eq!(requests(root), before, "nothing is recorded yet");
            let prompt = out["reserve"]["delegation"]["request"]["prompt"].as_str().unwrap();
            assert!(prompt.contains("Where does the request in `request` belong?") && prompt.contains("Where does `items.i0` belong?"), "both questions: {prompt}");
            let id = out["reserve"]["request_id"].as_str().unwrap().to_string();

            let again = write_triaged(&opts(&json!({ "reserve": id })), false, None, &|_, _, _| panic!("a pending reserve asks no Jev"));
            assert_eq!((again["reason"].clone(), again["reserve"]["request_id"].clone()), (json!("jev-reserve"), json!(id)), "{again}");
            assert_eq!(requests(root), before, "still pending");
            let log = store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap();
            let observed = crate::commands::spec_events::conversation::observed(root, "teste", &log);
            let reserves = observed.iter().filter(|e| e.event_type == "call" && e.str_field("command") == Some(reserve::RESERVE_CALL)).count();
            assert_eq!(reserves, 1, "one reserve, recorded once");

            let said = |choice: &str| json!({ "choice": choice, "certainty": certainty });
            let receipt = mustard_core::io::knowledge::enrichment::SessionReceipt {
                request_id: id.clone(),
                model: "inherit".into(),
                effort: None,
                result: json!({ "answers": { "q1": said("no"), "q2": said("project_pending"), "q3": said("new"), "q4": said("none") } }),
                input_tokens: None,
                output_tokens: None,
            };
            mustard_core::io::knowledge::enrichment::complete_session(root, root, &receipt).unwrap();
            let applied = write_triaged(&opts(&json!({ "reserve": id })), false, None, &|_, _, _| panic!("the receipt answers in place of the Jev"));
            assert_eq!(applied["ok"], json!(true), "{applied}");
            if place == "pending" {
                assert_eq!(applied["pending"], json!("P-1"), "{applied}");
                assert_eq!(requests(root), before, "the spec does not get it");
            } else {
                assert_eq!(requests(root), before + 1, "{applied}");
                assert!(applied["warnings"].to_string().contains("confirme com o usuário"), "an unsure receipt is a doubt: {applied}");
                let log = store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap();
                assert!(log.visible().iter().all(|event| !event.fields.contains_key("reserve")), "the reserve number stays out of the event");
            }
        }
    }

    /// Sem Jev, o pedido grava como sempre, sem aviso novo.
    #[test]
    fn a_request_without_jev_is_recorded_without_the_new_warning() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let opts = routed_request(root, "Trocar o tema do site para escuro.");
        let out = write_triaged(&opts(&json!({})), false, None, &|_, _, _| Asked::Off);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(out.get("warnings").is_none(), "{out}");
        assert_eq!(requests(root), 1);
    }

    /// A tarefa recusada, sozinha ou numa linha do lote, devolve o número do
    /// rascunho que guardou, e a correção com esse número e só o campo errado
    /// grava a tarefa inteira, passando pela conferência de sempre: a
    /// correção que erra de novo mantém o mesmo rascunho, o rascunho usado
    /// sai, e o número dele não vale mais nem para outro tipo de item.
    #[test]
    fn a_refused_task_keeps_a_draft_and_the_fix_sends_only_the_wrong_field() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = message(root, "user", "Quero a conferência de cada critério.");
        let crit = crate::shared::spec_state::seed_event(
            root,
            "teste",
            "criterion",
            json!({"when": "a obra fecha", "then": "cada critério é conferido", "proof": "git --version", "form": "ubiquitous"}),
        );
        let by_model = |event_type: &str, body: &Value| {
            let opts = WriteOpts { root: root.to_path_buf(), spec: Some("teste".into()), event_type: event_type.into(), json: body.to_string() };
            write_triaged(&opts, false, None, &|_, _, _| Asked::Off)
        };
        let log = || store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap();
        let without_files = json!({"title": "Fechamento confere cada critério", "text": "Conferir.", "agent": "- conferir cada critério",
            "depends_on": [], "covers": [crit], "origin": said});

        let refused = by_model("task", &without_files);
        assert_eq!(refused["reason"], json!("task-declaration-missing"), "{refused}");
        let draft = refused["draft"].as_u64().unwrap_or_else(|| panic!("a recusa devolve o número do rascunho: {refused}"));
        assert!(refused["draft_hint"].as_str().unwrap_or_default().contains(&format!("\"draft\": {draft}")), "a dica ensina a correção: {refused}");
        let before = log().events.len();

        let other = by_model("note", &json!({"draft": draft}));
        assert_eq!(other["reason"], json!("draft-other-type"), "o rascunho é de uma tarefa: {other}");
        let wrong = by_model("task", &json!({"draft": draft, "files": "src/a.rs"}));
        assert_eq!(wrong["ok"], json!(false), "{wrong}");
        assert_eq!(wrong["draft"], json!(draft), "a correção errada mantém o rascunho: {wrong}");
        assert_eq!(log().events.len(), before, "nada foi gravado");

        let fixed = by_model("task", &json!({"draft": draft, "files": []}));
        let id = fixed["id"].as_u64().unwrap_or_else(|| panic!("a correção só com o campo errado grava: {fixed}"));
        let log = log();
        let written = log.get(id).expect("a tarefa gravada");
        for field in ["title", "text", "agent", "depends_on", "covers"] {
            assert_eq!(written.fields.get(field), without_files.get(field), "{field} veio do rascunho: {:?}", written.fields);
        }
        assert_eq!(written.fields.get("files"), Some(&json!([])));
        assert!(written.fields.get("draft").is_none(), "o número do rascunho não vai ao evento: {:?}", written.fields);
        let used = by_model("task", &json!({"draft": draft, "files": []}));
        assert_eq!(used["reason"], json!("draft-unknown"), "o rascunho usado saiu: {used}");

        let line = |fields: &Value| {
            let mut line = fields.clone();
            line["type"] = json!("task");
            line.to_string()
        };
        let batch = |input: &str| batch_for(root, Some("teste"), input, None, &|_, _, _| Asked::Off);
        let refused_line = batch(&line(&without_files));
        let batch_draft = refused_line[0].1["draft"].as_u64().unwrap_or_else(|| panic!("a linha recusada devolve o rascunho: {refused_line:?}"));
        assert!(batch_draft > draft, "o número não volta a ser dado: {batch_draft}");
        assert_eq!(batch_line(refused_line[0].0, &refused_line[0].1)["draft"], json!(batch_draft), "a linha curta mantém o número");
        let fixed_line = batch(&line(&json!({"draft": batch_draft, "files": []})));
        assert_eq!(fixed_line[0].1["ok"], json!(true), "{fixed_line:?}");
    }

    /// A versão nova de uma tarefa que manda só os arquivos herda a parte do
    /// agente da anterior antes das conferências da porta: a prova que a parte
    /// herdada nomeia passa pela mesma conferência da prova mandada, e a que
    /// não roda é recusada sem gravar nada.
    #[test]
    fn a_new_task_version_checks_the_proof_named_in_what_it_inherits() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = message(root, "user", "Quero a conferência de cada critério.");
        let seed = |event_type: &str, fields: Value| crate::shared::spec_state::seed_event(root, "teste", event_type, fields);
        let crit = seed("criterion", json!({"when": "a obra fecha", "then": "cada critério é conferido", "proof": "git --version", "form": "ubiquitous"}));
        let old = seed("task", json!({"title": "Fechamento confere cada critério", "text": "Conferir.",
            "agent": "- conferir cada critério. Prova: `programa-que-nao-existe --rodar`", "files": [], "depends_on": [], "covers": [crit], "origin": said}));
        let events = || store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap().events.len();
        let before = events();

        let opts = WriteOpts { root: root.to_path_buf(), spec: Some("teste".into()), event_type: "task".into(),
            json: json!({"replaces": old, "files": [{"path": "src/b.rs"}]}).to_string() };
        let refused = write_triaged(&opts, false, None, &|_, _, _| Asked::Off);
        assert_eq!(refused["reason"], json!("proof-program-unknown"), "the inherited proof is checked: {refused}");
        assert_eq!(events(), before, "nothing is recorded");
    }
}
