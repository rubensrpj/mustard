//! A fila da rodada e as ondas em andamento: quais ondas saem agora, a
//! escolha do pedido antes do envio, a vaga fixa de cada uma, quais estão em
//! andamento, quais já estão entregues e
//! aprovadas, e o estado de cada uma que a página mostra. A rodada não pede
//! revisão de onda nenhuma: quem confere o trabalho, uma vez por obra, é o
//! agente de teste dedicado que o fechamento pede.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::normalize::Languages;
use mustard_core::domain::spec_events::{Block, BlockQuery, EventRef, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::domain::spec_index::title_of;
use mustard_core::domain::wave_prompt::{candidates, dispatch_items, recorded_choice, Candidates, Choice, TaskChoice};
use mustard_core::io::wave_prompt::{lesson_bank, wave_lessons};
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::leftovers::is_cleanup;
use super::report::tagged;
use super::stops::waves_replanned;
use crate::commands::flow::skill_search::{self, MAP_SUGGESTIONS};
use crate::commands::spec_events::write::{record, RecordCheck};
use crate::commands::wave::wave_overlap_check::{wave_graph, WaveGraph};
use crate::shared::dag::{sets_cross, touches_whole_tree};

/// Quantas ondas saem juntas quando o projeto não diz outra coisa: quatro, que
/// é quanto a máquina aguenta compilando ao mesmo tempo. Cada onda compila
/// dentro da própria vaga, e é o arquivo declarado que segura na fila a onda
/// pronta que cruzaria com outra.
const DEFAULT_PARALLEL: usize = 4;

/// Quantas ondas o projeto deixa compilar ao mesmo tempo.
pub(super) fn max_parallel(root: &Path) -> usize {
    mustard_core::ProjectConfig::load(root).max_compiling_waves().unwrap_or(DEFAULT_PARALLEL)
}

/// As ondas que saem nesta rodada: as que ainda não saíram nem entregaram,
/// cujas dependências já foram entregues, no máximo `limit` junto com as que
/// estão em andamento (`running`). Duas ondas cujos arquivos se cruzam — o
/// mesmo caminho, ou um padrão que casa o outro
/// ([`crate::shared::dag::files_cross`]) — nunca saem juntas, nem uma delas
/// sai por cima de uma onda em andamento que já declarou aquele arquivo: a
/// que perde a vez volta para a fila e espera quem está com o arquivo
/// entregar. A onda com o curinga da árvore inteira (`**`) nunca sai ao lado
/// de outra: nem com outra em andamento, nem com ela em andamento. A onda órfã — em andamento sem o
/// processo que a mandou — não conta como vaga ocupada para as outras
/// entrarem, porque ela não está compilando nada; a cópia dela é limpa à
/// parte, em [`open_copies`]. A onda parada pelo limite de consertos
/// (`stuck`) não sai, nem a que depende dela, direta ou por outra onda.
///
/// A onda de lote nasce sem dependência entre ondas, então uma dependência em
/// círculo não tem de onde vir pelo backlog, e o plano não a procura. O ciclo
/// sai da leitura do grafo ([`wave_graph`]) que esta função reaproveita —
/// nunca uma conta própria à parte —, e, se uma onda gravada antes do backlog
/// trouxer um, a rodada devolve os números do ciclo em vez de escolher uma
/// ordem às cegas.
pub(super) fn next_waves(
    log: &SpecLog,
    limit: usize,
    running: &BTreeMap<u64, u64>,
    stuck: &BTreeMap<u64, Vec<&SpecEvent>>,
) -> Result<Vec<u64>, Vec<u64>> {
    let graph = wave_graph(log);
    // A onda reprovada volta para a fila: sem isso o ciclo de conserto não
    // fecha, porque o fechamento recusa e diz qual refazer e a rodada nunca a
    // despacharia de novo.
    let to_redo = waves_to_redo(log);
    // O pedido gravado descreve o plano daquele momento: a onda que ganhou
    // versão nova depois dele, e ainda não entregou, sai de novo com o pedido
    // do plano atual.
    let replanned = waves_replanned(log);
    // O que já saiu da fila: a onda com pedido e também a onda que já
    // entregou. Só o pedido não basta, porque a onda entregue antes de a
    // rodada existir não tem pedido nenhum e apareceria como pronta para
    // sair — e sairia de novo um trabalho já feito.
    // Quais ondas já entregaram é pergunta do núcleo, e é ele que responde:
    // recalcular o filtro aqui deixava duas camadas decidindo a mesma coisa,
    // concordando hoje e livres para divergir amanhã.
    let delivered = log.delivered_waves();
    let already_out: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "send")
        .filter_map(SpecEvent::wave)
        .filter(|n| !replanned.contains(n))
        .chain(delivered.iter().copied())
        .filter(|n| !to_redo.contains(n))
        .collect();
    // Só a onda do plano é candidata: a de lote que ficou sem tarefa saiu
    // dele, e despachá-la seria um pedido sem nada dentro — o backlog já
    // reempacota numa onda nova a tarefa que ela perdeu.
    let planned = log.planned_waves();
    let mut depends: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for wave in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "wave") {
        if let Some(n) = wave.wave().filter(|n| planned.contains(n)) {
            depends.insert(n, wave.ints("depends_on"));
        }
    }
    let done = waves_done(log, running);
    // A onda órfã segue com o pedido aberto, mas não está compilando nada: a
    // vaga que ela guardava sozinha volta a valer para uma onda pronta de
    // verdade. Quem trava de fato quantas cópias saem é [`open_copies`], pela
    // vaga livre de verdade: cada vaga é uma cópia fixa, com a compilação
    // dentro dela, e a da órfã continua presa para o reenvio. Este número é
    // só o teto de quantas ondas prontas entram na disputa pelas vagas.
    let orphans = orphaned_waves(log);
    let effective_running = running.keys().filter(|n| !orphans.contains_key(n)).count();
    let slots = limit.saturating_sub(effective_running);
    // O arquivo que cada onda em andamento já declarou trava a vaga dela: uma
    // onda pronta cujo arquivo cruza com ele — o mesmo caminho, ou um padrão
    // que o casa — espera, mesmo com vaga livre, e a que sai primeiro nesta
    // rodada tranca o arquivo para a próxima da mesma leva.
    let mut taken: Vec<&String> = running.keys().flat_map(|n| graph.files.get(n).into_iter().flatten()).collect();
    // A onda com o curinga da árvore inteira cruza com todas, até com a que
    // não declara arquivo: com ela em andamento nada mais sai, e ela só sai
    // sem nenhuma outra em andamento nem saindo junto.
    let mut busy = !running.is_empty();
    let mut whole_tree_out = running.keys().any(|n| graph.files.get(n).is_some_and(touches_whole_tree));
    let mut go = Vec::new();
    for n in ready_in_order(&depends, &graph, &already_out, &delivered, &done)? {
        if go.len() >= slots || whole_tree_out {
            break;
        }
        if stuck.contains_key(&n) || dependencies_of(n, &depends).iter().any(|d| stuck.contains_key(d)) {
            continue;
        }
        let files: Vec<&String> = graph.files.get(&n).into_iter().flatten().collect();
        let whole_tree = touches_whole_tree(files.iter().copied());
        if (whole_tree && busy) || sets_cross(files.iter().copied(), taken.iter().copied()) {
            continue;
        }
        taken.extend(files);
        busy = true;
        whole_tree_out = whole_tree;
        go.push(n);
    }
    Ok(go)
}

// ---------------------------------------------------------------------------
// A escolha do pedido antes do envio
// ---------------------------------------------------------------------------

/// A linha em que o orquestrador devolve a escolha de uma onda.
pub(super) const ANALYSIS_LINE: &str = "ANALYSIS";

/// O que a linha `ANALYSIS` de uma onda trouxe: cada entrada que sai e cada
/// uma que entra, como veio — o item pelo código ou pelo número em `item`, a
/// lição pelo número do banco em `lesson` —, com o motivo.
pub(super) struct AnalysisLine {
    wave: u64,
    removed: Vec<(Value, String)>,
    added: Vec<(Value, String)>,
    /// A escolha da skill e dos arquivos de leitura, por tarefa, como a
    /// linha trouxe: cada entrada crua, ainda por validar contra as tarefas
    /// desta onda.
    tasks: Vec<Value>,
}

/// As linhas `ANALYSIS` do relatório `raw`. A que não se lê fica de fora com
/// um aviso, e a onda dela pede a escolha de novo: nada é recusado.
pub(super) fn analysis_lines(raw: Option<&str>, lang: Locale) -> (Vec<AnalysisLine>, Vec<Value>) {
    let mut lines = Vec::new();
    let mut warnings = Vec::new();
    for body in raw.map(|raw| tagged(raw, ANALYSIS_LINE)).unwrap_or_default() {
        let parsed = serde_json::from_str::<Value>(body).map_err(|e| e.to_string());
        match parsed.as_ref().ok().and_then(|fields| Some((fields, fields.get("wave")?.as_u64()?))) {
            Some((fields, wave)) => {
                let entries = |key: &str| -> Vec<(Value, String)> {
                    let listed = fields.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
                    listed
                        .iter()
                        .map(|entry| {
                            let why = entry.get("why").and_then(Value::as_str).map(str::trim).unwrap_or_default();
                            (entry.clone(), why.to_string())
                        })
                        .collect()
                };
                let tasks = fields.get("tasks").and_then(Value::as_array).cloned().unwrap_or_default();
                lines.push(AnalysisLine { wave, removed: entries("removed"), added: entries("added"), tasks });
            }
            None => {
                let detail = parsed.err().unwrap_or_else(|| body.to_string());
                let hint = translate("round.analysis_unreadable", lang).replace("{detail}", &detail);
                warnings.push(json!({ "reason": "analysis-unreadable", "hint": hint }));
            }
        }
    }
    (lines, warnings)
}

/// O que a escolha antes do envio decidiu para as ondas prontas.
pub(super) struct Analysed {
    /// As ondas que saem agora, na ordem da fila.
    pub go: Vec<u64>,
    /// A escolha de cada onda que sai com uma.
    pub choices: BTreeMap<u64, Choice>,
    /// Os candidatos de cada onda que espera a escolha do orquestrador.
    pub asked: Vec<Value>,
    /// As entradas da linha da escolha que ficaram como estavam.
    pub warnings: Vec<Value>,
}

/// A sugestão da rodada para uma tarefa, antes do envio: as skills que casam
/// com o texto dela pela busca, o "quando usar" de todas as skills da área
/// dela — mesmo as que a busca não achou — e até três arquivos parecidos do
/// mapa. A tarefa que já nomeia uma skill não pede sugestão de skill: ela já
/// está decidida, e só os arquivos continuam valendo.
struct TaskHint {
    task: u64,
    matched: Vec<String>,
    area: Vec<(String, String)>,
    files: Vec<String>,
}

impl TaskHint {
    /// `true` quando não há nada a sugerir para esta tarefa: nenhuma skill
    /// casou e nenhum arquivo parecido apareceu. O "quando usar" da área
    /// só é mostrado junto de uma das duas, nunca sozinho — quase todo
    /// projeto tem alguma skill na raiz, e mostrá-la sem motivo pediria a
    /// escolha do orquestrador em toda onda.
    fn is_empty(&self) -> bool {
        self.matched.is_empty() && self.files.is_empty()
    }
}

/// As sugestões de skill e de arquivos parecidos, por tarefa da onda `wave`,
/// que a rodada mostra ao orquestrador antes do envio: a mesma busca que o
/// plano usa na conferência ([`skill_search`]), e o mapa que o scan mantém em
/// dia depois de cada commit, nas línguas `languages`.
fn task_hints(root: &Path, log: &SpecLog, wave: u64, languages: &Languages) -> Vec<TaskHint> {
    log.block(BlockQuery::Wave(wave))
        .into_iter()
        .filter(|e| e.event_type == "task")
        .map(|task| {
            let text = task.str_field("text").unwrap_or_default();
            let named = task.str_field("skill").is_some_and(|s| !s.trim().is_empty());
            let on_disk = skill_search::skills_on_disk(root, std::slice::from_ref(&task));
            let matched = if named { Vec::new() } else { skill_search::matching_skills(&on_disk, text, languages) };
            let area = if named { Vec::new() } else { on_disk };
            let files = crate::commands::map::suggested_files(root, text, MAP_SUGGESTIONS, languages);
            TaskHint { task: task.id, matched, area, files }
        })
        .collect()
}

/// A escolha antes do envio de cada onda pronta (`ready`), antes de a cópia
/// dela ser criada. Os candidatos de cada onda são os itens do projeto todo,
/// os sem dono e as lições do banco que casam com ela, e a sugestão de skill
/// e de arquivos parecidos de cada tarefa dela; a onda sem nada disso sai
/// como hoje. A que tem sai com a escolha que a linha `ANALYSIS` do relatório
/// trouxe (`given`) ou, quando a mesma onda sai de novo sem plano novo, com a
/// escolha gravada no envio anterior dela, se essa escolha julgou cada
/// candidato de agora. Sem escolha, a onda não sai, e a resposta traz os
/// candidatos e as sugestões dela ao orquestrador, cada um com o título. Nada
/// é recusado, e nenhum agente é aberto para isso. As buscas cortam as
/// palavras nas línguas `languages`.
pub(super) fn analyse(
    root: &Path,
    log: &SpecLog,
    ready: &[u64],
    given: &[AnalysisLine],
    lang: Locale,
    languages: &Languages,
) -> Analysed {
    let replanned = waves_replanned(log);
    let codes = log.codes();
    let bank = lesson_bank(root);
    let mut out = Analysed { go: Vec::new(), choices: BTreeMap::new(), asked: Vec::new(), warnings: Vec::new() };
    for wave in ready.iter().copied() {
        let mut found = candidates(log, wave, languages);
        found.lessons = bank.as_ref().map(|bank| wave_lessons(bank, log, wave, languages)).unwrap_or_default();
        let hints = task_hints(root, log, wave, languages);
        if found.is_empty() && hints.iter().all(TaskHint::is_empty) {
            out.go.push(wave);
            continue;
        }
        let choice = match given.iter().rfind(|line| line.wave == wave) {
            Some(line) => Some(chosen(log, &codes, line, &found, lang, &mut out.warnings)),
            None => recorded_choice(log, wave)
                .filter(|choice| !replanned.contains(&wave) && choice.covers(&found))
                .map(|choice| choice.within(&found)),
        };
        match choice {
            Some(choice) => {
                out.choices.insert(wave, choice);
                out.go.push(wave);
            }
            None => out.asked.push(shown_candidates(log, &codes, wave, &found, &hints)),
        }
    }
    out
}

/// A escolha que a linha da onda traz, dentro dos candidatos de agora
/// (`found`): sai só o item do projeto todo ou a lição, entra só o item sem
/// dono, e cada um com o motivo. A entrada fora dos candidatos, ou sem
/// motivo, fica como estava, e um aviso diz qual.
fn chosen(
    log: &SpecLog,
    codes: &BTreeMap<u64, String>,
    line: &AnalysisLine,
    found: &Candidates,
    lang: Locale,
    warnings: &mut Vec<Value>,
) -> Choice {
    // A entrada aponta um item da spec em `item` ou uma lição do banco em
    // `lesson`: os números dos dois não se misturam.
    let find = |group: &[&SpecEvent], entry: &Value, lesson: bool| -> Option<u64> {
        let id = if lesson {
            entry.get("lesson")?.as_u64()?
        } else {
            match EventRef::from_value(entry.get("item")?)? {
                EventRef::Id(n) => log.current(n)?.id,
                EventRef::Code(code) => group.iter().find(|e| codes.get(&e.id) == Some(&code))?.id,
            }
        };
        group.iter().any(|e| e.id == id).then_some(id)
    };
    let mut pick = |group: &[&SpecEvent], entries: &[(Value, String)], lesson: bool| -> Vec<(u64, String)> {
        let mut out: Vec<(u64, String)> = Vec::new();
        for (entry, why) in entries.iter().filter(|(entry, _)| entry.get("lesson").is_some() == lesson) {
            match find(group, entry, lesson).filter(|_| !why.is_empty()) {
                Some(id) => {
                    if !out.iter().any(|(had, _)| *had == id) {
                        out.push((id, why.clone()));
                    }
                }
                None => warnings.push(ignored(line.wave, entry, lang)),
            }
        }
        out
    };
    let removed = pick(&found.project, &line.removed, false);
    let removed_lessons = pick(&found.lessons, &line.removed, true);
    let added = pick(&found.unowned, &line.added, false);
    // Lição não entra por escolha: as que casam com a onda já vão, e a lição
    // em `added` fica como estava.
    for (entry, _) in line.added.iter().filter(|(entry, _)| entry.get("lesson").is_some()) {
        warnings.push(ignored(line.wave, entry, lang));
    }
    // A escolha da skill e dos arquivos, por tarefa: a entrada aponta uma
    // tarefa desta onda, pelo código ou pelo número em `task`. A que não
    // aponta nenhuma fica de fora, com um aviso.
    let wave_tasks: Vec<&SpecEvent> =
        log.block(BlockQuery::Wave(line.wave)).into_iter().filter(|e| e.event_type == "task").collect();
    let mut tasks: Vec<TaskChoice> = Vec::new();
    for entry in &line.tasks {
        let task_id = entry
            .get("task")
            .and_then(EventRef::from_value)
            .and_then(|found| match found {
                EventRef::Id(n) => log.current(n).map(|e| e.id),
                EventRef::Code(code) => wave_tasks.iter().find(|e| codes.get(&e.id) == Some(&code)).map(|e| e.id),
            })
            .filter(|id| wave_tasks.iter().any(|e| e.id == *id));
        let Some(task_id) = task_id else {
            warnings.push(ignored(line.wave, entry, lang));
            continue;
        };
        let strings = |key: &str| -> Vec<String> {
            entry
                .get(key)
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        };
        tasks.push(TaskChoice {
            task: task_id,
            skills: strings("skills"),
            files: strings("files"),
        });
    }
    Choice { judged: found.ids(), removed, added, judged_lessons: found.lesson_ids(), removed_lessons, tasks }
}

/// O aviso da entrada da linha da escolha que ficou como estava: a frase diz
/// lição quando o número ignorado é de uma lição do banco, não de um item da
/// spec.
fn ignored(wave: u64, entry: &Value, lang: Locale) -> Value {
    let lesson = entry.get("lesson").is_some();
    let said = entry.get("item").or_else(|| entry.get("lesson")).or_else(|| entry.get("task")).unwrap_or(entry);
    let shown = said.as_str().map_or_else(|| said.to_string(), str::to_string);
    let key = if lesson { "round.analysis_ignored_lesson" } else { "round.analysis_ignored" };
    let hint = translate(key, lang).replace("{wave}", &wave.to_string()).replace("{item}", &shown);
    json!({ "reason": "analysis-item-ignored", "wave": wave, "hint": hint })
}

/// Os candidatos da onda `wave`, como a resposta da rodada os entrega ao
/// orquestrador: o título da onda, em cada grupo, cada candidato com o título
/// dele — o item pelo código, a lição pelo número do banco — e, por tarefa
/// que tem alguma sugestão, a skill que casa, o "quando usar" das skills da
/// área dela e os arquivos parecidos. O texto inteiro se lê pelo código.
fn shown_candidates(
    log: &SpecLog,
    codes: &BTreeMap<u64, String>,
    wave: u64,
    found: &Candidates,
    hints: &[TaskHint],
) -> Value {
    let title = |e: &SpecEvent| title_of(e).unwrap_or_default();
    let items = |group: &[&SpecEvent]| -> Vec<Value> {
        let code = |e: &SpecEvent| codes.get(&e.id).cloned().unwrap_or_else(|| e.id.to_string());
        group.iter().map(|e| json!({ "item": code(e), "title": title(e) })).collect()
    };
    let named = log.block(BlockQuery::Wave(wave)).into_iter().find(|e| e.event_type == "wave").map(title);
    let tasks: Vec<Value> = hints
        .iter()
        .filter(|hint| !hint.is_empty())
        .map(|hint| {
            let code = codes.get(&hint.task).cloned().unwrap_or_else(|| hint.task.to_string());
            let area: Vec<Value> =
                hint.area.iter().map(|(name, when)| json!({ "name": name, "when": when })).collect();
            json!({ "task": code, "matched_skills": hint.matched, "area_skills": area, "files": hint.files })
        })
        .collect();
    let mut out = json!({
        "wave": wave,
        "title": named.unwrap_or_default(),
        "project": items(&found.project),
        "unowned": items(&found.unowned),
        "lessons": found.lessons.iter().map(|e| json!({ "lesson": e.id, "title": title(e) })).collect::<Vec<_>>(),
    });
    if !tasks.is_empty() {
        out["tasks"] = json!(tasks);
    }
    out
}

/// As ondas com pedido aberto, cada uma com o número do pedido dela: a onda
/// tem pedido e nenhuma entrega depois dele. O pedido mais antigo que a
/// versão mais nova da onda ou de uma tarefa dela descreve um plano que já
/// mudou, e não conta. Uma onda que já entregou só volta a sair por uma
/// reprovação: o pedido que veio depois de uma entrega, sem reprovação entre
/// as duas, não é trabalho em curso, nem o pedido de uma onda que saiu do
/// plano. Não olha se o Claude Code que mandou o pedido segue aberto — é
/// [`waves_in_progress`] e [`orphaned_waves`] que decidem isso, cada uma para
/// o seu lado.
pub(crate) fn open_sends(log: &SpecLog) -> BTreeMap<u64, u64> {
    let planned = log.planned_waves();
    let replanned = waves_replanned(log);
    let verdicts = log.verdicts_by_wave();
    let mut deliveries: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for delivered in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "delivered") {
        if let Some(n) = delivered.wave() {
            deliveries.entry(n).or_default().push(delivered.id);
        }
    }
    log.last_by_wave("send")
        .into_iter()
        .filter(|(n, _)| planned.contains(n) && !replanned.contains(n))
        .filter(|(n, sent)| {
            let ids = deliveries.get(n).map(Vec::as_slice).unwrap_or_default();
            if ids.iter().any(|id| id > sent) {
                return false;
            }
            let judged_before = verdicts
                .get(n)
                .and_then(|list| list.iter().rev().find(|v| v.id < *sent))
                .and_then(|v| v.str_field("result"));
            !ids.iter().any(|id| id < sent) || judged_before == Some("rejected")
        })
        .collect()
}

/// O pedido de revisão aberto, pelo número dele: o último envio de revisão,
/// que o fechamento grava, sem veredito oficial depois dele. A volta que o
/// revisor gravou e ninguém assumiu ainda não fecha o pedido: só o veredito
/// que a rodada ou o fechamento grava ao assumi-la.
pub(crate) fn open_review(log: &SpecLog) -> Option<u64> {
    let visible = log.visible();
    let asked =
        visible.iter().filter(|e| e.event_type == "send" && e.str_field("role") == Some("review")).map(|e| e.id).max()?;
    (!visible.iter().any(|e| e.event_type == "verdict" && e.id > asked)).then_some(asked)
}

/// A versão nova do envio mais recente da onda `wave`: os campos dele, tirando
/// `v`, `id`, `code`, `at`, `type` e `search`, com `replaces` apontando para
/// ele e os campos de `extra` somados por cima. Usado quando a volta de uma
/// onda traz o consumo — o modelo usado de verdade, os passos e os tokens —,
/// que só se sabe depois do envio já gravado. `None` sem envio para a onda.
pub(crate) fn send_revision(log: &SpecLog, wave: u64, extra: Map<String, Value>) -> Option<Map<String, Value>> {
    let id = *log.last_by_wave("send").get(&wave)?;
    let event = log.get(id)?;
    let mut draft: Map<String, Value> = event
        .fields
        .iter()
        .filter(|(key, _)| !["v", "id", "code", "at", "type", "search"].contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    draft.insert("replaces".into(), json!(id));
    for (key, value) in extra {
        draft.insert(key, value);
    }
    Some(draft)
}

/// `true` quando o processo do Claude Code que mandou o envio `sent` ainda
/// está aberto: o par gravado (`claude_pid`, `claude_started`) segue vivo. Um
/// envio sem o par — de versão antiga, ou gravado fora do Linux — conta como
/// fechado; fora do Linux, onde nada é dado como órfão, conta como aberto:
/// quem decide aí é a pausa que o orquestrador manda.
fn claude_still_here(log: &SpecLog, sent: u64) -> bool {
    let Some(event) = log.get(sent) else { return false };
    match (event.int("claude_pid"), event.int("claude_started")) {
        #[allow(clippy::cast_possible_truncation)]
        (Some(pid), Some(started)) => crate::commands::flow::stuck::process_alive(pid as u32, started),
        _ => cfg!(not(target_os = "linux")),
    }
}

/// As ondas em andamento, cada uma com o número do pedido dela: as com pedido
/// aberto ([`open_sends`]) cujo Claude Code ainda está aberto.
pub(crate) fn waves_in_progress(log: &SpecLog) -> BTreeMap<u64, u64> {
    open_sends(log).into_iter().filter(|(_, sent)| claude_still_here(log, *sent)).collect()
}

/// As ondas órfãs, cada uma com o número do pedido dela: as com pedido aberto
/// ([`open_sends`]) cujo Claude Code já fechou. A rodada as reenvia com o
/// mesmo pedido de antes.
pub(crate) fn orphaned_waves(log: &SpecLog) -> BTreeMap<u64, u64> {
    open_sends(log).into_iter().filter(|(_, sent)| !claude_still_here(log, *sent)).collect()
}

/// A onda `n` é de lote — o binário a formou a partir do backlog, com autor
/// binário — e não a combinação à mão do plano: só a de lote devolve tarefa
/// ao backlog quando cortada, porque só ela é o que o backlog empacota de novo.
pub(crate) fn backlog_wave(log: &SpecLog, n: u64) -> bool {
    log.visible().into_iter().any(|event| {
        event.event_type == "wave" && event.wave() == Some(n) && event.str_field("author") == Some("binary")
    })
}

/// Minutos desde a última ação da onda `wave`: a mais nova entre a hora do
/// envio `sent` e a do arquivo de sinal de vida que
/// [`crate::hooks::observe::wave_alive_observer`] grava para a vaga que o
/// envio gravou. A vaga passa de uma onda para a seguinte, e o arquivo dela
/// pode guardar a hora da onda anterior: a hora do envio vale enquanto a onda
/// nova ainda não agiu. `None` sem nenhuma hora legível — a rodada não avisa
/// sem saber.
pub(crate) fn silent_minutes(root: &Path, spec: &str, wave: u64, log: &SpecLog, sent: u64) -> Option<i64> {
    let parse = |raw: &str| chrono::DateTime::parse_from_rfc3339(raw.trim()).ok();
    let send = log.get(sent);
    let copy = send.and_then(|event| event.str_field("copy").map(str::to_string)).or_else(|| {
        mustard_core::io::wave_prompt::recorded_copy(log, wave).map(|copy| copy.path)
    });
    let from_file = copy
        .filter(|copy| mustard_core::io::wave_prompt::is_slot_of(root, spec, copy))
        .and_then(|copy| Path::new(&copy).file_name().map(|name| name.to_string_lossy().into_owned()))
        .and_then(|slot| {
            std::fs::read_to_string(crate::hooks::observe::wave_alive_observer::alive_path(root, spec, &slot)).ok()
        })
        .and_then(|raw| parse(&raw));
    let from_send = send.and_then(|event| parse(event.at()));
    let at = match (from_file, from_send) {
        (Some(file), Some(send)) => file.max(send),
        (file, send) => file.or(send)?,
    };
    let now = chrono::Local::now().with_timezone(at.offset());
    Some((now - at).num_minutes())
}

/// As ondas do plano com o conserto pendente: a última revisão delas
/// reprovou, e nenhuma entrega chegou depois dessa reprovação. Só a entrega
/// resolve o conserto, com veredito novo ou sem ele: a aprovação final do
/// agente de teste dedicado responde pelo combinado inteiro e, quando não
/// aponta onda, é gravada sem onda nenhuma — não entra na revisão de onda
/// alguma e, por isso, não solta a onda reprovada. É a leitura única de
/// "onda com conserto pendente", que
/// `finished` (o fechamento), `waves_done` (a fila) e `wave_states` (o
/// estado da página) compartilham, para as três não discordarem de quando o
/// ciclo de conserto termina.
pub(crate) fn waves_pending_fix(log: &SpecLog) -> BTreeMap<u64, u64> {
    let delivered = log.last_by_wave("delivered");
    log.last_rejected().into_iter().filter(|(n, id)| delivered.get(n).is_none_or(|fix| fix < id)).collect()
}

/// As ondas entregues e aprovadas: têm entrega, não estão em andamento, e não
/// têm conserto pendente ([`waves_pending_fix`]). A onda entregue antes de a
/// rodada existir, sem pedido e sem veredito, está provada pelo código que
/// entrou. A rodada não pede revisão de onda nenhuma: a entrega já basta, e
/// só uma reprovação sem conserto ainda entregue devolve a onda para a fila.
fn waves_done(log: &SpecLog, running: &BTreeMap<u64, u64>) -> BTreeSet<u64> {
    let pending = waves_pending_fix(log);
    log.delivered_waves().into_iter().filter(|n| !running.contains_key(n) && !pending.contains_key(n)).collect()
}

/// A primeira onda planejada que ainda não está entregue e aprovada, com as
/// em andamento em `running`. `None` quando todas estão.
pub(super) fn first_unfinished(log: &SpecLog, running: &BTreeMap<u64, u64>) -> Option<u64> {
    let done = waves_done(log, running);
    log.planned_waves().into_iter().find(|n| !done.contains(n))
}

/// A onda `n` depende de todas as outras que ainda não terminaram: as
/// dependências dela, diretas ou por outra onda, alcançam cada onda planejada
/// que não está entregue e aprovada. É a última onda da obra.
fn depends_on_all(n: u64, depends: &BTreeMap<u64, Vec<u64>>, done: &BTreeSet<u64>) -> bool {
    let reached = dependencies_of(n, depends);
    depends.keys().filter(|w| **w != n && !done.contains(w)).all(|w| reached.contains(w))
}

/// As ondas de que `n` depende, diretas ou por outra onda.
fn dependencies_of(n: u64, depends: &BTreeMap<u64, Vec<u64>>) -> BTreeSet<u64> {
    let mut reached: BTreeSet<u64> = BTreeSet::new();
    let mut stack: Vec<u64> = depends.get(&n).cloned().unwrap_or_default();
    while let Some(on) = stack.pop() {
        if reached.insert(on) {
            stack.extend(depends.get(&on).cloned().unwrap_or_default());
        }
    }
    reached
}

/// As ondas que voltam para a fila: têm o conserto pendente
/// ([`waves_pending_fix`]) e ainda não foram despachadas de novo desde a
/// reprovação — o pedido mais novo da onda é anterior a essa reprovação.
/// Depois que o conserto sai, a onda espera a entrega dele, e não é
/// despachada de novo pela mesma reprovação — mas continua com o conserto
/// pendente para quem lê [`waves_pending_fix`], como o fechamento, até a
/// entrega chegar. O pedido do conserto que o plano da onda deixou para
/// trás, por uma versão nova da onda ou de uma tarefa dela, conta como se
/// não existisse: a onda volta para a fila e sai com o pedido do plano
/// atual.
pub(crate) fn waves_to_redo(log: &SpecLog) -> BTreeSet<u64> {
    let last_send = log.last_by_wave("send");
    let replanned = waves_replanned(log);
    waves_pending_fix(log)
        .into_iter()
        .filter(|(n, id)| replanned.contains(n) || last_send.get(n).is_none_or(|sent| sent < id))
        .map(|(n, _)| n)
        .collect()
}

/// As ondas prontas para sair, na ordem do grafo de dependências que
/// [`wave_graph`] já leu (`graph`) — por nível ([`WaveGraph::level`]) e,
/// entre as do mesmo nível, por quantas ondas cada uma destrava, direta ou
/// por outra, da maior para a menor, e por último pelo número. `already_out`
/// são as ondas que já saíram da fila e `delivered` as que já entregaram,
/// que é o que solta as ondas dependentes delas. A onda que depende de todas
/// as outras só sai com as dependências entregues e aprovadas (`done`).
///
/// Uma dependência em círculo é recusada com os números do ciclo
/// (`graph.cycle`) — a mesma leitura que já trava a aprovação do plano
/// ([`crate::commands::flow::plan`]), não uma segunda conta à parte que
/// pudesse discordar dela.
///
/// Só as ondas de `depends` são candidatas: quem chama o monta com as ondas
/// do plano ([`SpecLog::planned_waves`]), sem a de lote que ficou sem tarefa.
fn ready_in_order(
    depends: &BTreeMap<u64, Vec<u64>>,
    graph: &WaveGraph,
    already_out: &BTreeSet<u64>,
    delivered: &BTreeSet<u64>,
    done: &BTreeSet<u64>,
) -> Result<Vec<u64>, Vec<u64>> {
    if !graph.cycle.is_empty() {
        return Err(graph.cycle.clone());
    }
    let candidates: BTreeSet<u64> = depends
        .iter()
        .filter(|(n, _)| !already_out.contains(n))
        .filter(|(n, on)| {
            let released = if depends_on_all(**n, depends, done) { done } else { delivered };
            on.iter().all(|d| released.contains(d))
        })
        .map(|(n, _)| *n)
        .collect();
    let deps: BTreeMap<u64, BTreeSet<u64>> =
        depends.iter().map(|(n, on)| (*n, on.iter().copied().collect())).collect();
    let unlocks = task_unlocks(&deps);
    let mut order: Vec<(u32, std::cmp::Reverse<usize>, u64)> = depends
        .keys()
        .map(|&n| {
            (graph.level.get(&n).copied().unwrap_or(0), std::cmp::Reverse(unlocks.get(&n).copied().unwrap_or(0)), n)
        })
        .collect();
    order.sort_unstable();
    Ok(order.into_iter().map(|(_, _, n)| n).filter(|n| candidates.contains(n)).collect())
}

/// Os itens que o pedido de uma onda leva: os números de tudo que entrou
/// nele, com a escolha do orquestrador antes do envio (`choice`), nas
/// línguas `languages`.
pub(super) fn sent_items(log: &SpecLog, wave: u64, choice: Option<&Choice>, languages: &Languages) -> Vec<u64> {
    dispatch_items(log, wave, choice, languages).into_iter().map(|e| e.id).collect()
}

// ---------------------------------------------------------------------------
// O desempate de [`ready_in_order`] entre ondas do mesmo nível: quantas
// outras cada uma destrava
// ---------------------------------------------------------------------------

/// Quantas ondas cada uma destrava: o tamanho do fecho transitivo de quem
/// depende dela, direta ou por outra onda do backlog.
fn task_unlocks(deps: &BTreeMap<u64, BTreeSet<u64>>) -> BTreeMap<u64, usize> {
    let mut unlocks: BTreeMap<u64, usize> = deps.keys().map(|&n| (n, 0)).collect();
    for &m in deps.keys() {
        for on in task_transitive_deps(m, deps) {
            *unlocks.entry(on).or_insert(0) += 1;
        }
    }
    unlocks
}

/// As ondas de que `n` depende, diretas ou por outra, dentro de `deps`.
fn task_transitive_deps(n: u64, deps: &BTreeMap<u64, BTreeSet<u64>>) -> BTreeSet<u64> {
    let mut reached: BTreeSet<u64> = BTreeSet::new();
    let mut stack: Vec<u64> = deps.get(&n).cloned().unwrap_or_default().into_iter().collect();
    while let Some(on) = stack.pop() {
        if reached.insert(on) {
            stack.extend(deps.get(&on).cloned().unwrap_or_default());
        }
    }
    reached
}

/// Os arquivos que uma tarefa declara: cada item de `files` é o caminho, seja
/// como texto solto ou como `{"path": ...}`.
pub(super) fn task_files(task: &SpecEvent) -> BTreeSet<String> {
    task.fields
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| file.as_str().or_else(|| file.get("path").and_then(Value::as_str)))
        .map(|path| path.trim().replace('\\', "/"))
        .filter(|path| !path.is_empty())
        .collect()
}

/// A tarefa que `value` aponta, pelo número do evento ou pelo código, na
/// versão vigente dela — a mesma resolução que a gravação de uma tarefa nova
/// já faz para conferir o `depends_on` dela, só que lida aqui, não repetida à
/// parte por acaso: uma referência que não bate com tarefa nenhuma some.
fn backlog_task_ref(log: &SpecLog, codes: &BTreeMap<u64, String>, value: &Value) -> Option<u64> {
    let raw = match EventRef::from_value(value)? {
        EventRef::Id(id) => id,
        EventRef::Code(code) => log
            .visible()
            .into_iter()
            .filter(|event| event.event_type == "task" && codes.get(&event.id) == Some(&code))
            .map(|event| event.id)
            .next_back()?,
    };
    log.current(raw).filter(|event| event.event_type == "task").map(|event| event.id)
}

/// A versão nova da tarefa `id`: os campos dela, tirando `v`, `id`, `code`,
/// `at`, `type` e `search`, com `replaces` apontando para ela e os campos de
/// `extra` somados por cima — a mesma forma de [`send_revision`], para a
/// tarefa em vez do envio.
///
/// Na tarefa, `replaces` aponta a versão mais nova do mesmo código, ainda que
/// a leitura mostre outra: numa spec antiga, a remoção de só a versão com a
/// onda devolvia à leitura a versão sem onda, e a versão nova sobre ela
/// traria de volta a tarefa removida. Apontada a mais nova, a gravação recusa
/// a versão nova da tarefa que saiu. O número é o desta leitura, e não o
/// código resolvido na hora de gravar: a versão que outra gravação fizer
/// depois desta leitura continua recusando esta como substituída, e o texto
/// dela não se perde. O evento sem código, e o que não é tarefa, vão pelo
/// número, como antes.
fn task_revision(log: &SpecLog, id: u64, extra: Map<String, Value>) -> Option<Map<String, Value>> {
    let event = log.get(id)?;
    let mut draft: Map<String, Value> = event
        .fields
        .iter()
        .filter(|(key, _)| !["v", "id", "code", "at", "type", "search"].contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let newest = (event.event_type == "task")
        .then(|| log.codes())
        .and_then(|codes| {
            let code = codes.get(&id)?;
            log.events.iter().filter(|e| codes.get(&e.id) == Some(code)).map(|e| e.id).max()
        })
        .unwrap_or(id);
    draft.insert("replaces".into(), json!(newest));
    for (key, value) in extra {
        draft.insert(key, value);
    }
    Some(draft)
}

/// As tarefas vigentes da spec como o motor do backlog as lê: cada uma com as
/// dependências resolvidas para a versão vigente, os arquivos que declara e
/// se já está feita — a onda dela está entre as entregues e aprovadas
/// (`done_waves`).
fn backlog_population(log: &SpecLog, done_waves: &BTreeSet<u64>) -> Vec<crate::shared::dag::BacklogTask<u64>> {
    let codes = log.codes();
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "task")
        .map(|task| {
            let depends_on: BTreeSet<u64> = task
                .fields
                .get("depends_on")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|value| backlog_task_ref(log, &codes, value))
                .collect();
            let done = task.wave().is_some_and(|w| done_waves.contains(&w));
            crate::shared::dag::BacklogTask { id: task.id, depends_on, files: task_files(task), done }
        })
        .collect()
}

/// O que está no backlog: a tarefa sem onda nenhuma, e também a que carrega
/// um número de onda que nunca virou evento de onda — a spec antiga,
/// numerada à mão — desde que essa onda não tenha entregue. A tarefa de
/// onda com evento próprio, gravada pelo plano ou por um lote anterior,
/// nunca está aqui: quem a despacha é [`next_waves`]. A onda já entregue ou
/// aprovada fica como história, e a tarefa dela nunca volta para o backlog.
///
/// É a leitura única do backlog: a formação do lote ([`dispatch_backlog`]), o
/// próximo passo da rodada e o fechamento leem daqui, para os três não
/// discordarem de quando o backlog está vazio.
pub(crate) fn backlog_left(log: &SpecLog) -> BTreeSet<u64> {
    let running = waves_in_progress(log);
    let done_waves = waves_done(log, &running);
    let planned = log.planned_waves();
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "task")
        .filter(|t| match t.wave() {
            None => true,
            Some(w) => !done_waves.contains(&w) && !planned.contains(&w),
        })
        .map(|t| t.id)
        .collect()
}

/// As tarefas ainda por entregar: a do backlog ([`backlog_left`]) e a de onda
/// planejada que ainda não está entregue e aprovada. Não contam as das ondas
/// em `returning` — a que volta agora e as que o conserto dela fecha —,
/// porque a entrega as fecha agora.
///
/// É a leitura única de "tarefa ainda não entregue": a rodada a consulta
/// antes de criar tarefa para um item combinado que a volta não cumpriu.
pub(crate) fn tasks_not_delivered(log: &SpecLog, returning: &BTreeSet<u64>) -> BTreeSet<u64> {
    let running = waves_in_progress(log);
    let done_waves = waves_done(log, &running);
    let planned = log.planned_waves();
    let in_open_wave = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "task")
        .filter(|t| t.wave().is_some_and(|w| planned.contains(&w) && !done_waves.contains(&w)))
        .map(|t| t.id);
    backlog_left(log)
        .into_iter()
        .chain(in_open_wave)
        .filter(|id| log.get(*id).and_then(SpecEvent::wave).is_none_or(|w| !returning.contains(&w)))
        .collect()
}

/// `true` quando a limpeza ainda espera ([`is_cleanup`]): há onda do plano
/// por terminar — em andamento, por sair ou com conserto pendente
/// ([`first_unfinished`]) — ou tarefa do backlog (`left`) que não é
/// limpeza. A limpeza sai numa onda só, no fim da obra, quando nada mais
/// resta a fazer.
fn cleanup_waits(log: &SpecLog, running: &BTreeMap<u64, u64>, left: &BTreeSet<u64>) -> bool {
    first_unfinished(log, running).is_some()
        || left.iter().filter_map(|id| log.get(*id)).any(|task| !is_cleanup(task))
}

/// As tarefas do backlog ([`backlog_left`]) que já estão prontas — todas as
/// dependências entregues ou aprovadas —, na ordem em que o motor do backlog
/// as empacota. Vazia quando o backlog está vazio ou quando toda tarefa dele
/// ainda espera uma dependência. A limpeza só entra quando nada mais resta
/// ([`cleanup_waits`]): até lá ela não forma lote, e o próximo passo da
/// rodada não a oferece.
pub(crate) fn backlog_ready(log: &SpecLog) -> Vec<u64> {
    let running = waves_in_progress(log);
    let done_waves = waves_done(log, &running);
    let left = backlog_left(log);
    let held = cleanup_waits(log, &running, &left);
    crate::shared::dag::ready_tasks(&backlog_population(log, &done_waves))
        .into_iter()
        .filter(|id| left.contains(id))
        .filter(|id| !held || !log.get(*id).is_some_and(is_cleanup))
        .collect()
}

/// Forma os lotes prontos do backlog ([`backlog_left`], [`backlog_ready`]) — as
/// tarefas sem onda própria ainda, e também a tarefa de uma spec antiga que
/// carrega um número de onda sem evento de onda nenhum atrás dele e que
/// ainda não entregou — e grava o
/// evento de onda de cada um, com autor binário: os critérios são a união do
/// que as tarefas do lote cobrem (`covers`), o pronta-quando é a prova
/// desses critérios, ligadas por " && " quando são mais de uma, e a ordem de
/// despacho do lote fica no campo que a onda já reservava para isso
/// (`order`). Cada tarefa do lote ganha, na mesma passada, uma versão nova
/// com o número da onda formada — o número velho, de spec antiga, é
/// ignorado — e é isso que liga o lote ao resto da leitura, que só conhece
/// onda pelo `n`/`wave` gravado em cada evento. A onda já entregue ou
/// aprovada fica como história, e a tarefa dela nunca volta para cá.
///
/// Na mesma chamada, [`stale_batch_revisions`] atualiza o lote já formado
/// que perdeu alguma tarefa para um evento de remoção depois de gravado —
/// sem isso o pedido dele abriria pelo `done_when` congelado na formação,
/// citando texto de tarefa que já não existe.
///
/// Grava tudo junto ou nada: as versões dos lotes já formados, cada onda
/// nova e a versão de cada tarefa dela são montadas primeiro e conferidas em
/// sequência pela mesma conferência da gravação ([`RecordCheck`]), sobre o
/// arquivo como as anteriores o deixariam; só então vão ao arquivo. Uma
/// recusa na conferência não deixa onda gravada sem as tarefas dela.
///
/// A prontidão e o empacotamento são o mesmo motor de [`crate::shared::dag`]
/// que já prova, sozinho, o desempate e o teto de trabalho do lote: esta
/// função só lê a spec, monta a população de tarefas e grava o que ele
/// decidiu. Além das prontas, o motor recebe as tarefas que esperam
/// (`waiting`): a que espera só por tarefas do mesmo lote, ou já entregues, e
/// divide arquivo com ele entra no lote, depois delas. E recebe os arquivos
/// das ondas do plano ainda abertas (`busy`): a parte que cruza um deles vai
/// para um lote só dela, que espera a onda aberta sem segurar outra parte.
/// `Ok(vec![])` sem tarefa pronta no backlog.
///
/// A limpeza ([`is_cleanup`]) nunca entra num lote comum: sai da ordem e da
/// espera, para a dependente que entra no lote nunca a puxar. Quando ela está
/// pronta — no fim da obra, ver [`backlog_ready`] —, todas as limpezas saem
/// num lote só, depois dos outros e fora do teto do lote.
///
/// `run_round_with_mine` (`apps/rt/src/commands/flow/round/answer.rs`) chama
/// esta função antes de formar a lista de ondas prontas: quem decide as
/// ondas prontas para o pedido ([`next_waves`], acima) só lê o evento de
/// onda já gravado, e é por isso que o lote precisa existir como onda antes
/// de `next_waves` rodar, na mesma chamada.
///
/// Recebe duas leituras da spec. `on_entry` é a de quando a rodada começou,
/// antes do relatório dela mexer em onda ou tarefa: a tarefa que o corte de
/// uma onda de lote devolve solta agora mesmo não está pronta nela, e só
/// empacota na rodada seguinte, nunca na mesma que a soltou. `locked` é a
/// leitura feita já com a trava do passo do git presa, que vê o que outra
/// rodada, chegada ao mesmo tempo, gravou antes desta pegar a trava. O lote
/// leva só a tarefa pronta nas duas — a versão vigente, em `locked`, da que
/// estava pronta na entrada —, e todo o resto (o número da onda nova, a
/// versão de cada tarefa, o lote já formado a atualizar) sai de `locked`:
/// a tarefa que a outra rodada já empacotou tem, ali, a onda dela, e não
/// volta a sair numa onda com o mesmo número. A tarefa que espera segue a
/// mesma regra das duas leituras: só entra a que está no backlog nas duas.
///
/// # Errors
///
/// A recusa da conferência, antes de qualquer gravação, ou a da primeira
/// gravação que falhar.
pub(crate) fn dispatch_backlog(start: &Path, spec: &str, on_entry: &SpecLog, locked: &SpecLog) -> Result<Vec<u64>, mustard_core::domain::spec_events::Refusal> {
    use crate::shared::dag::{pack_batches, Batch, BATCH_CAP};

    let log = locked;
    let running = waves_in_progress(log);
    let done_waves = waves_done(log, &running);
    let by_id: BTreeMap<u64, &SpecEvent> =
        log.visible().into_iter().filter(|e| e.event_type == "task").map(|t| (t.id, t)).collect();
    let cleanup = |id: &u64| by_id.get(id).is_some_and(|task| is_cleanup(task));
    let population = backlog_population(log, &done_waves);
    // A versão vigente, em `locked`, do que a leitura de entrada via.
    let in_locked = |ids: BTreeSet<u64>| -> BTreeSet<u64> {
        ids.into_iter().filter_map(|id| log.current(id)).map(|task| task.id).collect()
    };
    let ready_on_entry = in_locked(backlog_ready(on_entry).into_iter().collect());
    let (tidy, order): (Vec<u64>, Vec<u64>) =
        backlog_ready(log).into_iter().filter(|id| ready_on_entry.contains(id)).partition(|id| cleanup(id));
    let left_on_entry = in_locked(backlog_left(on_entry));
    let waiting: Vec<u64> = backlog_left(log)
        .into_iter()
        .filter(|id| left_on_entry.contains(id) && !order.contains(id) && !cleanup(id))
        .collect();
    // O arquivo de cada onda do plano que ainda não terminou: em andamento,
    // por sair ou com conserto pendente.
    let graph = wave_graph(log);
    let busy: BTreeSet<String> = log
        .planned_waves()
        .into_iter()
        .filter(|n| !done_waves.contains(n))
        .flat_map(|n| graph.files.get(&n).cloned().unwrap_or_default())
        .collect();
    let mut writes: Vec<(&str, Map<String, Value>)> =
        stale_batch_revisions(log).into_iter().map(|revised| ("wave", revised)).collect();
    let mut batches =
        if order.is_empty() { Vec::new() } else { pack_batches(&population, &order, &waiting, &busy, BATCH_CAP) };
    if !tidy.is_empty() {
        let files = tidy.iter().filter_map(|id| by_id.get(id)).flat_map(|task| task_files(task)).collect();
        batches.push(Batch { tasks: tidy, files });
    }

    // O número segue o maior já gravado, com a onda de lote que ficou vazia
    // incluída: ela saiu do plano, mas o número dela não volta a nascer.
    let mut next_n = log.last_wave_number();
    let mut formed = Vec::new();
    for batch in &batches {
        next_n += 1;
        let tasks: Vec<&SpecEvent> = batch.tasks.iter().filter_map(|id| by_id.get(id).copied()).collect();
        // A tarefa nascida de uma onda cobre critério, com prova para juntar
        // aqui; a que nasce do item combinado sem onda dona (o backlog da
        // revisão final) cobre o item, que não tem prova — o texto de quem
        // marcou o item sem atender é o que diz quando a onda entrega.
        let fields = mustard_core::domain::wave_prompt::backlog_fields(log, &tasks);
        let draft = json!({
            "n": next_n,
            "text": fields.text,
            "criteria": fields.criteria,
            "done_when": fields.done_when,
            "order": batch.tasks,
            "author": "binary",
        });
        let Value::Object(draft) = draft else { unreachable!("json! de um mapa sempre é objeto") };
        writes.push(("wave", draft));
        for id in &batch.tasks {
            if let Some(revised) = task_revision(log, *id, Map::from_iter([("wave".to_string(), json!(next_n))])) {
                writes.push(("task", revised));
            }
        }
        formed.push(next_n);
    }
    if writes.is_empty() {
        return Ok(formed);
    }
    let mut check = RecordCheck::open(start, spec, PhaseWriter::Binary)?;
    for (event_type, draft) in &writes {
        check.record(event_type, draft.clone())?;
    }
    for (event_type, draft) in writes {
        record(start, spec, event_type, draft, PhaseWriter::Binary)?;
    }
    Ok(formed)
}

/// A versão nova do registro de cada onda de lote (autor `binary`) que ainda
/// não foi enviada, quando uma tarefa dela saiu do backlog por evento de remoção
/// depois de o lote ter sido formado: sem isso o pedido abriria pelo
/// `done_when` congelado na formação, que pode citar o texto de uma tarefa
/// que não existe mais, mesmo com a lista de tarefas do pedido já saindo
/// certa. Recalcula critério, texto e pronto-quando
/// ([`mustard_core::domain::wave_prompt::backlog_fields`]) a partir das
/// tarefas que a leitura de agora mostra visíveis naquela onda — o mesmo
/// conjunto que alimenta a lista de tarefas do pedido — e monta uma versão
/// nova só quando a ordem gravada perdeu alguma tarefa; quem grava é
/// [`dispatch_backlog`], junto dos lotes novos. A onda já enviada
/// fica intocada: o pedido dela já foi montado, e mudar o registro não muda
/// o que o agente já recebeu. A onda que perdeu todas as tarefas fica de
/// fora: sem tarefa nenhuma ela saiu do plano
/// ([`SpecLog::planned_waves`]), e não há o que recalcular.
fn stale_batch_revisions(log: &SpecLog) -> Vec<Map<String, Value>> {
    let sent: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "send")
        .filter_map(SpecEvent::wave)
        .collect();
    let mut revisions = Vec::new();
    for n in log.planned_waves() {
        if sent.contains(&n) || !backlog_wave(log, n) {
            continue;
        }
        let items = log.block(BlockQuery::Wave(n));
        let Some(wave_event) = items.iter().copied().find(|e| e.event_type == "wave") else { continue };
        let tasks: Vec<&SpecEvent> = items.iter().copied().filter(|e| e.event_type == "task").collect();
        if tasks.is_empty() {
            continue;
        }
        // A ordem gravada aponta os números de antes da tarefa ganhar a
        // versão nova com o `wave` (`task_revision`, acima): segue a cadeia
        // de substituição até a versão vigente de cada uma, e só conta como
        // viva a que ainda está entre as tarefas visíveis desta onda.
        let recorded_order = wave_event.ints("order");
        let live_ids: BTreeSet<u64> = tasks.iter().map(|t| t.id).collect();
        let live_order: Vec<u64> =
            recorded_order.iter().filter_map(|id| log.current(*id)).map(|t| t.id).filter(|id| live_ids.contains(id)).collect();
        if live_order.len() == recorded_order.len() {
            continue;
        }
        let fields = mustard_core::domain::wave_prompt::backlog_fields(log, &tasks);
        let extra = Map::from_iter([
            ("text".to_string(), json!(fields.text)),
            ("criteria".to_string(), json!(fields.criteria)),
            ("done_when".to_string(), json!(fields.done_when)),
            ("order".to_string(), json!(live_order)),
        ]);
        revisions.extend(task_revision(log, wave_event.id, extra));
    }
    revisions
}

/// O estado de cada onda do plano ([`SpecLog::planned_waves`]), pela mesma
/// leitura que decide o que a rodada despacha: em andamento, reprovada na
/// última revisão, entregue e aprovada — a rodada não pede revisão de onda
/// nenhuma, então a entrega já vale como aprovada — ou por fazer, a que o
/// backlog formou e ainda não saiu inclusive. A onda de lote que ficou sem
/// tarefa saiu do plano e não aparece, nem com a entrega gravada em nome
/// dela. A página da spec lista as ondas por aqui, e só por aqui.
pub(crate) fn wave_states(log: &SpecLog) -> mustard_core::view::document::WaveStates {
    use mustard_core::view::document::WaveState;
    let running = waves_in_progress(log);
    let rejected = waves_pending_fix(log);
    let done = waves_done(log, &running);
    log.planned_waves()
        .into_iter()
        .map(|n| {
            let state = if running.contains_key(&n) {
                WaveState::Running
            } else if rejected.contains_key(&n) {
                WaveState::Rejected
            } else if done.contains(&n) {
                WaveState::Approved
            } else {
                WaveState::Todo
            };
            (n, state)
        })
        .collect()
}

/// O cenário da volta com tarefa não feita, que os testes da entrega também
/// usam.
#[cfg(test)]
pub(super) use tests::{return_with_an_undone_task, task_now, UndoneReturn};

#[cfg(test)]
mod tests {
    use std::process::Command;

    use std::path::PathBuf;

    use mustard_core::io::spec_events as store;
    use mustard_core::io::wave_prompt::{shown, slot_path};
    use tempfile::tempdir;

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// A cópia gravada no envio da onda `wave`.
    fn sent_copy(root: &Path, wave: u64) -> String {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        mustard_core::io::wave_prompt::recorded_copy(&log, wave).unwrap_or_else(|| panic!("wave {wave}")).path
    }

    /// Duas ondas sem dependência e sem arquivo em comum saem juntas, cada
    /// uma na sua vaga — a pasta fixa `a`, `b`… sob a pasta da spec —,
    /// posta no commit atual. O pedido de cada uma traz a cópia e nunca uma
    /// pasta de compilação, em projeto Node ou Rust: o que a cópia compila
    /// fica dentro dela. O teto de compilações do projeto limita quantas
    /// saem.
    #[test]
    fn two_waves_without_a_shared_file_go_out_together_each_in_its_own_slot_and_the_cap_holds() {
        for kind in ["npm", "cargo"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
            std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
            mapped(root, kind);

            let out = round(root, "x", None);
            assert_eq!(waves_in(&out, "dispatch"), vec![1, 2], "sem arquivo em comum, as duas saem juntas: {out}");
            let head = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output().unwrap();
            let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
            for (at, wave) in [1_u64, 2].iter().enumerate() {
                let copy = sent_copy(root, *wave);
                let expected = mustard_core::io::wave_prompt::slot_path(root, "x", at);
                assert_eq!(copy, mustard_core::io::wave_prompt::shown(&expected), "{kind}: {out}");
                assert!(expected.join(".git").is_file(), "the copy of wave {wave} is a linked checkout");
                assert_eq!(std::fs::read_to_string(expected.join("src/a.rs")).unwrap(), "fn um() {}\n");
                let copy_head = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(&expected).output().unwrap();
                assert_eq!(String::from_utf8_lossy(&copy_head.stdout).trim(), head, "the copy stands on the current commit");
                let prompt = out["dispatch"][at]["prompt"].as_str().unwrap_or_default();
                assert!(prompt.contains(&format!("`{copy}`")), "{prompt}");
                for word in ["CARGO_TARGET_DIR", "target/copias", "pasta de compilação"] {
                    assert!(!prompt.contains(word), "{kind}: no build folder in the request ({word}): {prompt}");
                }
            }
        }

        // Com o teto do projeto em 1, só uma onda sai por rodada.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        let out = round(root, "x", None);
        assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{out}");
    }

    /// A rodada abre a cópia da onda fora da pasta do projeto, na pasta das
    /// cópias dele — o nome do projeto e um código curto que não muda —, sob
    /// a pasta da spec, e o envio grava esse caminho, que o pedido cita. O
    /// revisor final usa a vaga da última onda; nada nasce em
    /// `.claude/worktrees`. A entrega que cita o arquivo pelo caminho absoluto
    /// da cópia gravada no envio volta relativa ao repositório, mesmo quando
    /// essa cópia não é a que a pasta das cópias daria hoje, como a da onda
    /// que saiu antes de a pasta mudar.
    #[test]
    fn a_wave_copy_is_born_outside_the_project_folder() {
        use mustard_core::io::wave_prompt::{copies_dir, final_copy_path};

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");

        let project = std::fs::canonicalize(root).unwrap();
        let copies = copies_dir(root);
        assert!(!copies.starts_with(root) && !copies.starts_with(&project), "{copies:?}");
        let folder = copies.file_name().unwrap().to_string_lossy().into_owned();
        let (name, code) = folder.rsplit_once('-').unwrap();
        assert_eq!(name, project.file_name().unwrap().to_string_lossy(), "{folder}");
        assert!(code.len() == 8 && code.chars().all(|c| c.is_ascii_hexdigit()), "{folder}");
        assert_eq!(copies_dir(&project), copies, "the same project always gives the same folder");

        let copy = sent_copy(root, 1);
        assert_eq!(copy, shown(&copies.join("x").join("a")), "{out}");
        assert!(Path::new(&copy).join(".git").is_file(), "the copy is a linked checkout");
        let prompt = out["dispatch"][0]["prompt"].as_str().unwrap_or_default();
        assert!(prompt.contains(&format!("`{copy}`")), "{prompt}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(final_copy_path(root, "x", &log), copies.join("x").join("a"), "the review uses the wave's slot");
        assert!(!root.join(".claude").join("worktrees").exists(), "nothing is born inside the project");

        // Um envio mais novo da onda grava a cópia noutro lugar.
        let elsewhere = tempdir().unwrap();
        let moved = shown(&elsewhere.path().join("x").join("a"));
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let sent = log.visible().into_iter().rfind(|e| e.wave() == Some(1) && e.event_type == "send").unwrap();
        let draft = json!({
            "wave": 1, "role": "wave", "text": sent.str_field("text").unwrap_or_default(),
            "lines": sent.int("lines").unwrap_or(1), "chars": sent.int("chars").unwrap_or(1),
            "items": sent.fields.get("items").cloned().unwrap_or_else(|| json!([])),
            "mustard": "0", "author": "binary", "copy": moved,
        });
        store::write(&path, "send", draft.as_object().cloned().unwrap(), &[]).unwrap();

        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// mudou\n").unwrap();
        let body = json!({"wave": 1, "text": "Saiu.", "files": [format!("{moved}/src/a.rs")], "commit": "a onda 1 saiu"});
        let wrote = returned(root, body);
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let log = store::read(&path).unwrap().unwrap();
        let back = log.events.iter().rfind(|e| e.event_type == "delivered").unwrap();
        assert_eq!(back.fields.get("files"), Some(&json!(["src/a.rs"])), "{wrote}");
    }

    /// Duas ondas sem dependência entre si, mas com o mesmo arquivo
    /// declarado, não saem juntas mesmo com vaga livre: quem chegou depois
    /// espera a que já está em andamento entregar, e só então sai sozinha.
    /// Quatro ondas sem arquivo em comum saem juntas, até o novo teto de
    /// quatro; a quinta espera a vaga.
    #[test]
    fn trava_por_arquivo_impede_ondas_com_arquivo_em_comum_e_libera_ate_quatro_sem_cruzar() {
        // Duas ondas com o mesmo arquivo: só a que já está em andamento sai,
        // mesmo com vaga livre para a outra.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1, 3], "a onda 2 divide arquivo com a 1 e espera: {out}");

        let again = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&again, "dispatch"), vec![2], "livre o arquivo, a onda 2 sai sozinha: {again}");

        // Quatro ondas sem arquivo em comum saem juntas, até o teto de
        // quatro; a quinta espera.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(
            root,
            "x",
            &[
                (1, &["src/a.rs"], &[]),
                (2, &["src/b.rs"], &[]),
                (3, &["src/c.rs"], &[]),
                (4, &["src/d.rs"], &[]),
                (5, &["src/e.rs"], &[]),
            ],
        );
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1, 2, 3, 4], "sem cruzar arquivo, as quatro saem: {out}");
    }

    /// A cópia da onda órfã — pedido aberto sem o processo que mandou —
    /// volta suja ao commit atual sozinha, na mesma rodada em que a rodada
    /// nota a órfã, sem que ninguém peça o reenvio primeiro: quem falhou no
    /// meio do trabalho não deixou uma retomada em curso. A limpeza leva o
    /// arquivo novo e o mudado, mas não a pasta de compilação que o git
    /// ignora: a compilação feita ali fica para quem usar a vaga depois. A
    /// órfã segura a vaga dela: a onda que sai ao lado vai para outra.
    #[cfg(target_os = "linux")]
    #[test]
    fn copia_orfa_volta_limpa_ao_commit_atual_sem_esperar_reenvio() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        std::fs::write(root.join(".git").join("info").join("exclude"), "target/\n").unwrap();
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");

        let copy_path = sent_copy(root, 1);
        let copy = Path::new(&copy_path);
        std::fs::write(copy.join("src/a.rs"), "fn retomada() {}\n").unwrap();
        std::fs::write(copy.join("src/novo.rs"), "fn novo() {}\n").unwrap();
        let build = copy.join("target").join("debug").join("compilado.o");
        std::fs::create_dir_all(build.parent().unwrap()).unwrap();
        std::fs::write(&build, "compilado").unwrap();
        assert_ne!(git_text(copy, &["status", "--porcelain"]), "", "a cópia precisa estar suja antes da rodada");

        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let sent = log.visible().into_iter().rfind(|e| e.wave() == Some(1) && e.event_type == "send").unwrap();
        let mut draft = json!({
            "wave": 1, "role": "wave", "text": sent.str_field("text").unwrap_or_default(),
            "lines": sent.int("lines").unwrap_or(1), "chars": sent.int("chars").unwrap_or(1),
            "items": sent.fields.get("items").cloned().unwrap_or_else(|| json!([])),
            "mustard": "0", "author": "binary", "copy": sent.str_field("copy").unwrap_or_default(),
        });
        drop(log);

        let mut dead = Command::new("true").spawn().expect("spawn the fixture process");
        let dead_pid = dead.id();
        dead.wait().expect("reap the fixture process");
        draft["claude_pid"] = json!(dead_pid);
        draft["claude_started"] = json!(1);
        store::write_at(
            &path,
            "send",
            draft.as_object().cloned().unwrap(),
            &[],
            &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
        )
        .unwrap();

        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        let second = round(root, "x", None);

        assert_eq!(git_text(copy, &["status", "--porcelain"]), "", "a cópia órfã volta limpa sem pedir reenvio");
        let head = git_text(root, &["rev-parse", "HEAD"]);
        assert_eq!(git_text(copy, &["rev-parse", "HEAD"]), head, "a cópia volta ao commit atual: {head}");
        assert_eq!(std::fs::read_to_string(&build).unwrap(), "compilado", "a compilação ignorada fica na vaga");
        assert_eq!(waves_in(&second, "dispatch"), vec![2, 1], "a onda 2 sai, e a órfã é reenviada: {second}");
        let next = shown(&mustard_core::io::wave_prompt::slot_path(root, "x", 1));
        assert_eq!(sent_copy(root, 2), next, "a órfã segura a vaga a, e a onda 2 vai para a b: {second}");
        assert_eq!(sent_copy(root, 1), copy_path, "a órfã volta na mesma vaga: {second}");
    }

    /// A onda que já entregou não é despachada de novo, mesmo sem pedido
    /// nenhum: a onda entregue antes de a rodada existir não tem pedido, e
    /// mandá-la sair seria mandar refazer um trabalho já feito.
    #[test]
    fn a_wave_that_already_delivered_does_not_go_out_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        write(
            root,
            "x",
            "delivered",
            json!({"wave": 1, "text": "A onda 1 saiu antes da rodada.", "files": ["src/a.rs"]}),
        );

        let out = round(root, "x", None);
        let waves: Vec<u64> =
            out["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(waves, vec![2], "a onda 1 já tem entrega: {out}");
    }

    /// A onda cuja última revisão reprovou volta a ser despachada, e uma vez
    /// só: depois que o conserto sai, a mesma reprovação não a manda de novo.
    /// Entregue o conserto, ele para de estar em andamento e de sair de novo
    /// — falta o agente de teste dedicado, no fechamento, dizer se ficou
    /// certo, e a rodada não pede revisão nenhuma dele.
    #[test]
    fn a_rejected_wave_goes_out_again_and_only_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(1), "{first}");

        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        let again = round(root, "x", Some(&verdict(root, 1, "rejected", "faltou o teste")));
        assert_eq!(again["dispatch"].as_array().map(Vec::len), Some(1), "a onda reprovada volta a sair: {again}");

        let quiet = round(root, "x", None);
        assert_eq!(quiet["dispatch"], json!([]), "o conserto já saiu: {quiet}");

        let back = round(root, "x", Some(&delivered(root, 1, "O teste entrou.", &["src/a.rs"])));
        assert_eq!(back["ok"], json!(true), "{back}");
        assert_eq!(back["dispatch"], json!([]), "o conserto não sai de novo: {back}");
        assert!(back.get("reviews").is_none(), "a rodada não pede revisão do conserto: {back}");
    }

    /// A onda reprovada cujo conserto já saiu e que ganha uma tarefa nova
    /// depois desse envio volta para a fila, como se o envio não existisse: a
    /// rodada seguinte a despacha de novo, com a tarefa nova no pedido, e ela
    /// fica em andamento com o envio novo; a rodada depois dessa não a solta
    /// outra vez.
    #[test]
    fn a_rejected_wave_that_gets_a_new_task_after_its_fix_went_out_goes_out_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        let fix = round(root, "x", Some(&verdict(root, 1, "rejected", "faltou o teste")));
        assert_eq!(waves_in(&fix, "dispatch"), vec![1], "{fix}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let said = log.visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
        let task = write(root, "x", "task", json!({"wave": 1, "text": "Tarefa nova da onda 1.",
            "files": [{"path": "src/b.rs"}], "depends_on": [], "origin": said}));
        let task_code = task["code"].as_str().unwrap_or_else(|| panic!("{task}")).to_string();

        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![1], "the replanned fix goes out again: {again}");
        let prompt = again["dispatch"][0]["prompt"].as_str().unwrap_or_default();
        assert!(prompt.lines().any(|l| l.starts_with(&format!("- `{task_code}`"))), "{prompt}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let newest = log.visible().into_iter().rfind(|e| e.event_type == "send").map(|e| codes[&e.id].clone()).unwrap();
        let running = again["running"].as_array().cloned().unwrap_or_default();
        assert_eq!(running.len(), 1, "{again}");
        assert_eq!(running[0]["wave"], json!(1), "{again}");
        assert_eq!(running[0]["send"], json!(newest), "{again}");

        let quiet = round(root, "x", None);
        assert_eq!(waves_in(&quiet, "dispatch"), Vec::<u64>::new(), "{quiet}");
        assert_eq!(waves_in(&quiet, "running"), vec![1], "{quiet}");
    }

    /// O reenvio de uma onda replanejada usa a cópia que já existe, de um
    /// envio anterior: limpa, ela vai para o commit atual do checkout
    /// principal, mesmo que um commit alheio à rodada tenha avançado o
    /// checkout desde a criação dela; com mudança sem commitar, como a de uma
    /// retomada em andamento, ela fica no commit que estava.
    #[test]
    fn a_clean_copy_moves_to_the_current_commit_before_a_send() {
        for dirty in [false, true] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[])]);
            let first = round(root, "x", None);
            assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
            let copy_path = sent_copy(root, 1);
            let copy = Path::new(&copy_path);
            let old_head = git_text(copy, &["rev-parse", "HEAD"]);

            // Um commit alheio à rodada, de outro trabalho no checkout
            // principal, avança o HEAD sem tocar a onda 1, que segue sem
            // entrega e com a cópia dela ainda no disco.
            std::fs::write(root.join("src/b.rs"), "fn dois() {}\n").unwrap();
            git_at(root, &["add", "-A"]);
            git_at(root, &["commit", "-q", "-m", "outro trabalho"]);
            let new_head = git_text(root, &["rev-parse", "HEAD"]);
            assert_ne!(old_head, new_head, "o commit alheio precisa mudar o HEAD");

            if dirty {
                std::fs::write(copy.join("src/a.rs"), "fn retomada() {}\n").unwrap();
            }

            // A onda 1 ganha versão nova e volta para a fila com a cópia que
            // já existe, sem entrega nem reprovação nenhuma antes disso.
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let planned = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(1)).unwrap();
            let mut fields = planned.fields.clone();
            for key in ["v", "id", "code", "at", "search", "type", "author"] {
                fields.remove(key);
            }
            let mut wave = Value::Object(fields);
            wave["done_when"] = json!("A suíte passa e o teste novo também.");
            wave["replaces"] = json!(planned.id);
            write(root, "x", "wave", wave);

            let again = round(root, "x", None);
            assert_eq!(waves_in(&again, "dispatch"), vec![1], "a onda replanejada volta a sair: {again}");
            assert_eq!(sent_copy(root, 1), copy_path, "a replanejada volta à vaga que gravou: {again}");
            let copy_head = git_text(copy, &["rev-parse", "HEAD"]);
            if dirty {
                assert_eq!(copy_head, old_head, "a cópia com mudança fica como está: {copy_head}");
                assert_eq!(std::fs::read_to_string(copy.join("src/a.rs")).unwrap(), "fn retomada() {}\n");
            } else {
                assert_eq!(copy_head, new_head, "a cópia limpa vai para o commit atual: {copy_head}");
            }
        }
    }

    /// A data de mudança do arquivo `path`.
    fn modified(path: &Path) -> std::time::SystemTime {
        std::fs::metadata(path).and_then(|meta| meta.modified()).unwrap_or_else(|err| panic!("{path:?}: {err}"))
    }

    /// Põe no arquivo `path` uma data antiga e redonda, que o teste reconhece
    /// depois.
    fn age(path: &Path) -> std::time::SystemTime {
        let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::File::options().write(true).open(path).unwrap().set_modified(old).unwrap();
        old
    }

    /// A vaga passa de uma onda para a seguinte sem nascer de novo: a onda 2
    /// sai na vaga que a 1 deixou, posta no commit novo, e só o arquivo que
    /// o commit da onda 1 mudou ganha data nova — o outro guarda a dele, e a
    /// compilação que o git ignora fica. O pedido da onda 2 diz que a vaga é
    /// reaproveitada e lista o arquivo que mudou desde o último uso dela.
    #[test]
    fn two_preparations_of_the_same_slot_keep_the_date_of_an_unchanged_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        std::fs::write(root.join(".git").join("info").join("exclude"), "target/\n").unwrap();
        std::fs::write(root.join("mustard.json"), br#"{"prepareCommand":"true"}"#).unwrap();
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let slot = slot_path(root, "x", 0);
        assert_eq!(sent_copy(root, 1), shown(&slot), "{first}");
        let fresh = translate("prompt.execution.prepare_new", Locale::PtBr).replace("{command}", "true");
        assert!(first["dispatch"][0]["prompt"].as_str().unwrap_or_default().contains(&fresh), "a vaga nova pede o preparo");
        let untouched = age(&slot.join("src/b.rs"));
        age(&slot.join("src/a.rs"));
        let build = slot.join("target").join("compilado.o");
        std::fs::create_dir_all(build.parent().unwrap()).unwrap();
        std::fs::write(&build, "compilado").unwrap();

        let second = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&second, "dispatch"), vec![2], "{second}");
        assert_eq!(sent_copy(root, 2), shown(&slot), "a onda 2 usa a vaga que a 1 deixou: {second}");
        assert_eq!(git_text(&slot, &["rev-parse", "HEAD"]), git_text(root, &["rev-parse", "HEAD"]));
        assert!(std::fs::read_to_string(slot.join("src/a.rs")).unwrap().contains("A soma saiu."), "{second}");
        assert_ne!(modified(&slot.join("src/a.rs")), untouched, "o arquivo mudado ganha data nova");
        assert_eq!(modified(&slot.join("src/b.rs")), untouched, "o arquivo que não mudou guarda a data");
        assert_eq!(std::fs::read_to_string(&build).unwrap(), "compilado", "a compilação ignorada fica na vaga");
        let prompt = second["dispatch"][0]["prompt"].as_str().unwrap_or_default();
        let reused = translate("prompt.execution.prepare_reused", Locale::PtBr);
        let opening = reused.split("{files}").next().unwrap_or_default();
        assert!(prompt.contains(opening) && prompt.contains("`src/a.rs`"), "{prompt}");
        assert!(!prompt.contains("`src/b.rs`."), "só o que mudou entra na lista: {prompt}");
    }

    /// A onda replanejada, cujo último envio ficou sem entrega, volta à vaga
    /// que esse envio gravou, mesmo com outra vaga livre antes dela na
    /// ordem.
    #[test]
    fn a_replanned_wave_goes_back_to_the_slot_it_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
        let second_slot = shown(&slot_path(root, "x", 1));
        assert_eq!(sent_copy(root, 2), second_slot, "{first}");

        round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        replan(root, 2);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![2], "{again}");
        assert_eq!(sent_copy(root, 2), second_slot, "a vaga a está livre, e a onda 2 volta à b: {again}");
    }

    /// A vaga cuja pasta existe sem ser cópia do git — sobra de um processo
    /// que caiu no meio — nasce de novo, sem o que havia nela; a vaga cuja
    /// pasta sumiu com o registro do git ainda de pé também, sem que o
    /// registro velho trave a cópia nova.
    #[test]
    fn a_slot_folder_that_is_not_a_git_copy_or_a_stale_registration_is_rebuilt() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        let slot = slot_path(root, "x", 0);
        std::fs::create_dir_all(slot.join("lixo")).unwrap();
        std::fs::write(slot.join("lixo").join("velho.txt"), "sobra").unwrap();

        let first = round(root, "x", None);
        assert_eq!(sent_copy(root, 1), shown(&slot), "{first}");
        assert!(slot.join(".git").is_file() && !slot.join("lixo").exists(), "a pasta solta vira cópia: {first}");

        std::fs::remove_dir_all(&slot).unwrap();
        assert!(git_text(root, &["worktree", "list", "--porcelain"]).contains(&shown(&slot)), "o registro ficou");
        let second = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&second, "dispatch"), vec![2], "o registro velho não trava a cópia: {second}");
        assert_eq!(sent_copy(root, 2), shown(&slot), "{second}");
        assert!(slot.join(".git").is_file(), "{second}");
        assert_eq!(git_text(&slot, &["rev-parse", "HEAD"]), git_text(root, &["rev-parse", "HEAD"]));
    }

    /// Os avisos de arquivo local que não chegou à cópia, na resposta da
    /// rodada: o item da lista de cada um.
    fn local_files_missing(out: &Value) -> Vec<String> {
        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        warnings
            .iter()
            .filter(|w| w["reason"] == json!("local-file-missing"))
            .map(|w| w["file"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    /// A cópia nova da onda recebe cada arquivo da lista de arquivos locais
    /// do projeto (`localFiles`) no mesmo caminho, como arquivo comum e com o
    /// mesmo conteúdo, nunca como link: mudar o da cópia não muda o do
    /// repositório principal. O arquivo que falta no principal, o caminho com
    /// `..` e o absoluto não chegam, viram aviso cada um, e a cópia sai do
    /// mesmo jeito — sem escrever nada fora dela. A cópia limpa reaproveitada,
    /// depois da limpeza que apaga o que o git ignora, recebe os arquivos de
    /// novo, com o conteúdo de agora. Com a lista vazia ou ausente, nada é
    /// copiado e nada é avisado.
    #[test]
    fn a_new_copy_gets_the_local_files_as_copies_never_links() {
        use mustard_core::io::wave_prompt::copies_dir;

        let dir = tempdir().unwrap();
        let root = &dir.path().join("projeto");
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\nconfig/local.json\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join("config/local.json"), "{\"porta\":1}\n").unwrap();
        let outside = dir.path().join("fora.env");
        std::fs::write(&outside, "FORA=1\n").unwrap();
        let absolute = shown(&dir.path().join("absoluto.env"));
        std::fs::write(&absolute, "ABSOLUTO=1\n").unwrap();
        let listed = json!([".env", "config/local.json", "falta.env", "../fora.env", absolute]);
        std::fs::write(root.join("mustard.json"), json!({ "localFiles": listed }).to_string()).unwrap();

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "a cópia sai mesmo com o arquivo que falta: {out}");
        let copy_path = sent_copy(root, 1);
        let copy = Path::new(&copy_path);
        for (file, content) in [(".env", "SEGREDO=1\n"), ("config/local.json", "{\"porta\":1}\n")] {
            let kind = std::fs::symlink_metadata(copy.join(file)).unwrap().file_type();
            assert!(kind.is_file() && !kind.is_symlink(), "{file} chega como arquivo comum");
            assert_eq!(std::fs::read_to_string(copy.join(file)).unwrap(), content, "{file}");
        }
        std::fs::write(copy.join(".env"), "MUDOU=1\n").unwrap();
        assert_eq!(std::fs::read_to_string(root.join(".env")).unwrap(), "SEGREDO=1\n", "a cópia não escreve no principal");
        assert!(!copies_dir(root).join("fora.env").exists(), "o item com .. não escreve fora da cópia");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "FORA=1\n");
        assert_eq!(std::fs::read_to_string(&absolute).unwrap(), "ABSOLUTO=1\n", "o item absoluto não toca o arquivo");
        assert_eq!(local_files_missing(&out), ["falta.env", "../fora.env", absolute.as_str()], "{out}");
        let hints = out["warnings"].as_array().cloned().unwrap_or_default();
        let hint = hints.iter().find(|w| w["file"] == json!("falta.env")).unwrap()["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`falta.env`") && hint.contains(&copy_path), "{hint}");
        assert_eq!(git_text(copy, &["status", "--porcelain", "--untracked-files=all"]), "", "a cópia segue limpa");

        // A cópia limpa, como a limpeza da cópia órfã a deixa, perde o que o
        // git ignora; a onda sai de novo com a mesma cópia, que recebe os
        // arquivos com o conteúdo de agora.
        git_at(copy, &["clean", "-fdx"]);
        assert!(!copy.join(".env").exists());
        std::fs::write(root.join(".env"), "SEGREDO=2\n").unwrap();
        replan(root, 1);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![1], "{again}");
        assert_eq!(sent_copy(root, 1), copy_path, "a mesma cópia: {again}");
        let kind = std::fs::symlink_metadata(copy.join(".env")).unwrap().file_type();
        assert!(kind.is_file() && !kind.is_symlink(), "{again}");
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=2\n", "{again}");
        assert_eq!(std::fs::read_to_string(copy.join("config/local.json")).unwrap(), "{\"porta\":1}\n");

        // Lista vazia ou ausente: a cópia sai sem nenhum arquivo local.
        for config in [json!({ "localFiles": [] }), json!({})] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            std::fs::create_dir_all(root).unwrap();
            std::fs::write(root.join(".gitignore"), ".env\n").unwrap();
            approved(root, "x", &[(1, &["src/a.rs"], &[])]);
            std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
            std::fs::write(root.join("mustard.json"), config.to_string()).unwrap();
            let out = round(root, "x", None);
            assert_eq!(waves_in(&out, "dispatch"), vec![1], "{config}: {out}");
            let copy = sent_copy(root, 1);
            assert!(!Path::new(&copy).join(".env").exists(), "{config}: nada é copiado");
            assert!(local_files_missing(&out).is_empty(), "{config}: {out}");
        }
    }

    /// A cópia reaproveitada com um link no lugar do arquivo local recebe um
    /// arquivo comum, com o conteúdo de agora da pasta principal: o link sai
    /// antes da cópia, e o alvo dele fica como estava.
    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_a_local_file_gives_way_to_a_plain_copy() {
        let dir = tempdir().unwrap();
        let root = &dir.path().join("projeto");
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
        std::fs::write(root.join("mustard.json"), json!({ "localFiles": [".env"] }).to_string()).unwrap();
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let copy_path = sent_copy(root, 1);
        let copy = Path::new(&copy_path);

        let target = dir.path().join("alvo.env");
        std::fs::write(&target, "ALVO=1\n").unwrap();
        std::fs::remove_file(copy.join(".env")).unwrap();
        std::os::unix::fs::symlink(&target, copy.join(".env")).unwrap();
        std::fs::write(root.join(".env"), "SEGREDO=2\n").unwrap();
        replan(root, 1);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![1], "{again}");
        assert_eq!(sent_copy(root, 1), copy_path, "a mesma cópia: {again}");
        let kind = std::fs::symlink_metadata(copy.join(".env")).unwrap().file_type();
        assert!(kind.is_file() && !kind.is_symlink(), "o link saiu: {again}");
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=2\n", "{again}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "ALVO=1\n", "o alvo do link fica como estava");
        assert!(local_files_missing(&again).is_empty(), "{again}");
    }

    /// O arquivo local que já está igual na vaga não é escrito de novo: a
    /// onda seguinte na mesma vaga o encontra com a data de antes. O que
    /// mudou na pasta principal chega com o conteúdo novo.
    #[test]
    fn an_identical_local_file_keeps_its_date_in_the_slot() {
        let dir = tempdir().unwrap();
        let root = &dir.path().join("projeto");
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\nlocal.json\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
        std::fs::write(root.join("local.json"), "{}\n").unwrap();
        std::fs::write(root.join("mustard.json"), json!({ "localFiles": [".env", "local.json"] }).to_string()).unwrap();
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let copy = PathBuf::from(sent_copy(root, 1));
        let kept = age(&copy.join(".env"));
        age(&copy.join("local.json"));
        std::fs::write(root.join("local.json"), "{\"porta\":2}\n").unwrap();

        let second = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(sent_copy(root, 2), shown(&copy), "{second}");
        assert_eq!(modified(&copy.join(".env")), kept, "o arquivo local igual guarda a data: {second}");
        assert_eq!(std::fs::read_to_string(copy.join("local.json")).unwrap(), "{\"porta\":2}\n");
        assert_ne!(modified(&copy.join("local.json")), kept, "o que mudou chega de novo");
    }

    /// Um arquivo versionado posto à mão na lista de arquivos locais não é
    /// copiado: a cópia fica com a versão do commit, mesmo com a pasta
    /// principal mudada, e a rodada o devolve na lista dos que não foram
    /// copiados. O arquivo da mesma lista que o git ignora chega.
    #[test]
    fn a_versioned_file_in_the_local_list_keeps_the_commit_version_in_the_copy() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\n").unwrap();
        std::fs::write(root.join("config/app.json"), "{\"versao\":\"commit\"}\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join("config/app.json"), "{\"versao\":\"principal\"}\n").unwrap();
        std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
        let listed = json!({ "localFiles": [".env", "config/app.json"] });
        std::fs::write(root.join("mustard.json"), listed.to_string()).unwrap();

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let copy_path = sent_copy(root, 1);
        let copy = Path::new(&copy_path);
        let app = std::fs::read_to_string(copy.join("config/app.json")).unwrap();
        assert_eq!(app, "{\"versao\":\"commit\"}\n", "a versão do commit fica intacta: {out}");
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=1\n", "{out}");
        assert_eq!(local_files_missing(&out), ["config/app.json"], "{out}");
        assert_eq!(git_text(copy, &["status", "--porcelain", "--untracked-files=all"]), "", "a cópia segue limpa");
    }

    /// As cópias que um teste da rodada cria moram fora da pasta temporária
    /// dele, e saem quando ele termina — também quando ele falha —, e o git
    /// do projeto deixa de listá-las.
    #[test]
    fn the_copies_a_test_makes_leave_when_it_ends_even_when_it_fails() {
        use mustard_core::io::wave_prompt::copies_dir;

        for fails in [false, true] {
            let dir = tempdir().unwrap();
            let root = dir.path().to_path_buf();
            let inside = root.clone();
            let ended = std::thread::spawn(move || {
                approved(&inside, "x", &[(1, &["src/a.rs"], &[])]);
                let out = round(&inside, "x", None);
                let copy = sent_copy(&inside, 1);
                assert!(Path::new(&copy).join(".git").is_file(), "{out}");
                assert!(!fails, "o teste falhou depois de criar a cópia");
            })
            .join();
            assert_eq!(ended.is_err(), fails);
            assert!(!copies_dir(&root).exists(), "fails={fails}: a pasta das cópias saiu");
            let listed = git_text(&root, &["worktree", "list", "--porcelain"]);
            assert_eq!(listed.matches("worktree ").count(), 1, "fails={fails}: só o checkout principal: {listed}");
        }
    }

    /// O estado de cada onda que a página mostra acompanha a rodada: por
    /// fazer antes de sair, em andamento depois do pedido, aprovada assim que
    /// entrega — a rodada não pede revisão nenhuma —, o que já solta a onda
    /// seguinte, reprovada por quem julgar (o agente de teste dedicado, no
    /// fechamento), em andamento de novo com o conserto e aprovada no fim.
    #[test]
    fn the_wave_states_follow_the_round() {
        use mustard_core::view::document::WaveState::{Approved, Rejected, Running, Todo};
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        let events = root.join(".claude/spec/x/spec.ndjson");
        let states = || {
            let log = mustard_core::io::spec_events::read(&events).unwrap().unwrap();
            wave_states(&log).into_iter().collect::<Vec<_>>()
        };
        assert_eq!(states(), [(1, Todo), (2, Todo)], "nothing went out yet: every wave of the plan is to do");
        round(root, "x", None);
        assert_eq!(states(), [(1, Running), (2, Todo)]);
        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(states(), [(1, Approved), (2, Running)], "the delivery already frees the next wave");

        // A reprovação, que só o agente de teste dedicado grava, no
        // fechamento.
        let rejected = json!({"author": "review", "final": true, "wave": 1, "result": "rejected", "text": "faltou o teste"});
        mustard_core::io::spec_events::write(&events, "verdict", rejected.as_object().cloned().unwrap(), &[]).unwrap();
        assert_eq!(states(), [(1, Rejected), (2, Running)]);
        round(root, "x", None);
        assert_eq!(states(), [(1, Running), (2, Running)], "the fix went out");
        round(root, "x", Some(&delivered(root, 1, "O teste entrou.", &["src/a.rs"])));
        let approval = json!({"author": "review", "final": true, "wave": 1, "result": "approved", "text": "pronto"});
        mustard_core::io::spec_events::write(&events, "verdict", approval.as_object().cloned().unwrap(), &[]).unwrap();
        assert_eq!(states(), [(1, Approved), (2, Running)]);
    }

    /// O estado das ondas lista só as do plano: a onda de lote que o backlog
    /// formou e ainda não saiu vem como por fazer, e a onda de lote que ficou
    /// sem tarefa não vem, nem com uma entrega gravada em nome dela.
    #[test]
    fn the_wave_states_list_only_the_planned_waves() {
        use mustard_core::view::document::WaveState::Todo;
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let events = root.join(".claude/spec/x/spec.ndjson");
        let raw = |kind: &str, fields: Value| {
            mustard_core::io::spec_events::write(&events, kind, fields.as_object().cloned().unwrap(), &[]).unwrap();
        };
        let log = mustard_core::io::spec_events::read(&events).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap().id;
        let batch = |n: u64| {
            json!({"author": "binary", "n": n, "text": format!("Lote {n}."), "criteria": [crit],
                "done_when": "A suíte passa."})
        };
        // O lote 2 ficou sem tarefa, e uma entrega foi gravada em nome dele.
        raw("wave", batch(2));
        raw("delivered", json!({"author": "wave", "wave": 2, "text": "Saiu.", "files": ["src/b.rs"]}));
        // O lote 3 tem tarefa e ainda não saiu.
        raw("wave", batch(3));
        raw("task", json!({"author": "binary", "wave": 3, "text": "Tarefa do lote.", "files": [{"path": "src/c.rs"}],
            "depends_on": []}));
        let log = mustard_core::io::spec_events::read(&events).unwrap().unwrap();
        assert_eq!(
            wave_states(&log).into_iter().collect::<Vec<_>>(),
            [(1, Todo), (3, Todo)],
            "the empty batch stays out, and the formed batch that did not go out is to do"
        );
    }

    /// A onda em andamento — com pedido e sem entrega depois dele — ocupa uma
    /// vaga do limite e a cópia fixa dela: a rodada não passa do limite
    /// contando as que já saíram, e a onda que sai no lugar da que voltou
    /// fica com a vaga livre, e não com a da que segue em andamento. A cópia
    /// da que voltou continua no disco depois do commit, pronta para a
    /// seguinte.
    /// O pedido anterior ao replanejamento da onda não conta como andamento.
    /// A resposta lista as ondas em andamento com o código do pedido de cada
    /// uma.
    #[test]
    fn a_wave_in_flight_holds_its_slot_and_a_send_before_the_replan_does_not_count() {
        // A onda 2 volta; a 3 sai no lugar dela, enquanto a 1 segue.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
        let held = sent_copy(root, 1);
        let freed = sent_copy(root, 2);
        let out = round(root, "x", Some(&delivered(root, 2, "Saiu.", &["src/b.rs"])));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(Path::new(&freed).join(".git").is_file(), "a cópia fica depois do commit da onda: {out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![3], "a vaga da 2 ficou livre: {out}");
        assert_eq!(sent_copy(root, 3), freed, "a 3 usa a vaga que a 2 deixou, e não a da 1 ({held})");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let sent = |wave: u64| {
            log.visible()
                .into_iter()
                .rfind(|e| e.event_type == "send" && e.wave() == Some(wave))
                .map(|e| codes[&e.id].clone())
                .unwrap()
        };
        let running: Vec<Value> = out["running"].as_array().cloned().unwrap_or_default();
        assert_eq!(
            running.iter().map(|r| (r["wave"].clone(), r["send"].clone())).collect::<Vec<_>>(),
            vec![(json!(1), json!(sent(1))), (json!(3), json!(sent(3)))],
            "{out}"
        );

        // Duas ondas em andamento enchem o limite de duas.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        let full = round(root, "x", None);
        assert_eq!(waves_in(&full, "dispatch"), Vec::<u64>::new(), "as duas vagas estão ocupadas: {full}");
        assert_eq!(waves_in(&full, "running"), vec![1, 2], "{full}");

        // Replanejada depois do pedido, a onda 2 deixa de estar em andamento:
        // ela sai de novo, e a vaga dela não fica presa ao pedido velho.
        replan(root, 2);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![2], "{again}");
        let running = again["running"].as_array().cloned().unwrap_or_default();
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let newest = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "send" && e.wave() == Some(2))
            .map(|e| codes[&e.id].clone())
            .next_back()
            .unwrap();
        assert_eq!(running[1]["wave"], json!(2), "o pedido novo é o que conta: {again}");
        assert_eq!(running[1]["send"], json!(newest), "o pedido novo é o que conta: {again}");

        // A onda 1 entregou antes de a rodada existir, e um pedido saiu para
        // ela depois, sem reprovação no meio: ela não está em andamento, e as
        // duas vagas ficam para as outras.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);
        write(root, "x", "delivered", json!({"wave": 1, "text": "Saiu antes da rodada.", "files": ["src/a.rs"]}));
        seed_send(root, 1);
        let free = round(root, "x", None);
        assert_eq!(waves_in(&free, "dispatch"), vec![2, 3], "{free}");
        assert_eq!(waves_in(&free, "running"), vec![2, 3], "a onda 1 não está em andamento: {free}");
    }

    /// A onda que depende de todas as outras só sai depois de elas estarem
    /// aprovadas, não só entregues — a rodada não pede revisão nenhuma, então
    /// a entrega já vale como aprovação, menos para a onda que voltou
    /// reprovada: essa segura a que depende de todas até o conserto voltar. A
    /// que depende de uma parte sai com a entrega, como antes.
    #[test]
    fn the_wave_that_depends_on_all_the_others_waits_for_their_approvals() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[1, 2])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);

        // As duas entregam: sem revisão nenhuma da rodada, a entrega já basta
        // para soltar a 3.
        let both = format!("{}\n{}", delivered(root, 1, "Saiu.", &["src/a.rs"]), delivered(root, 2, "Saiu.", &["src/b.rs"]));
        let out = round(root, "x", Some(&both));
        assert!(out.get("reviews").is_none(), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![3], "as duas entregas já soltam a 3: {out}");

        // A onda 2 depende só da 1, e a 3 não passa por ela: a 2 sai com a
        // entrega da 1.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1]), (3, &["src/c.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 3]);
        let out = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "a entrega basta para quem não depende de todas: {out}");

        // A onda 2 entrega e é reprovada antes de a 1 entregar: a 3, que
        // depende das duas, não sai enquanto a 2 não voltar aprovada, mesmo
        // com a 1 pronta.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[1, 2])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        round(root, "x", None);
        round(root, "x", Some(&delivered(root, 2, "Saiu.", &["src/b.rs"])));
        let events = root.join(".claude/spec/x/spec.ndjson");
        let rejected = json!({"author": "review", "final": true, "wave": 2, "result": "rejected", "text": "faltou algo"});
        mustard_core::io::spec_events::write(&events, "verdict", rejected.as_object().cloned().unwrap(), &[]).unwrap();
        let out = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "a reprovada volta antes da que depende de todas: {out}");
        assert_eq!(waves_in(&out, "running"), vec![2], "a 3 ainda espera a 2 aprovada: {out}");
    }

    /// As linhas dos itens combinados que a escolha da onda 1 deixa no
    /// pedido, cada uma com o bloco, o código e o título, e embaixo dela,
    /// recuada, a parte do agente do item.
    const CHOSEN_ITEMS: &str = "- `agreed` MSTD-RULE-0001: Vale sempre: a tabela nova tem chave.\n  - conferir pelo teste\n\
        - `agreed` MSTD-RULE-0003: Vale sempre: a tabela nova tem índice.\n  - conferir pelo teste\n\
        - `agreed` MSTD-DEC-0001: Sem dono: a tabela nasce vazia.\n  - conferir pelo teste\n\
        - `agreed` MSTD-DEC-0003: Da onda um: a coluna é texto.\n  - conferir pelo teste\n";

    /// A spec aprovada da análise antes do envio: uma onda, com a tarefa que
    /// faz uma das regras do projeto todo, duas regras do projeto todo que
    /// ela não faz, dois itens sem dono, que a palavra-chave liga à tarefa, e
    /// uma decisão da onda. Devolve o
    /// número de cada item, pelo código.
    fn with_items_to_judge(root: &Path) -> BTreeMap<String, u64> {
        approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            let rule = |text: &str| {
                id_of(&write(root, "x", "rule", json!({"title": text, "text": text, "example": "e", "keys": ["k"],
                    "applies_to": {"files": ["**"]}, "origin": said})))
            };
            rule("Vale sempre: a tabela nova tem chave.");
            rule("Vale sempre: a spec vira um PR só.");
            let done = rule("Vale sempre: a tabela nova tem índice.");
            for (text, extra) in [("Sem dono: a tabela nasce vazia.", json!({"keys": ["tabela"]})),
                ("Sem dono: o download não muda.", json!({"keys": ["índice"]})),
                ("Da onda um: a coluna é texto.", json!({"waves": [1]}))] {
                let mut body = json!({"title": text, "text": text, "keys": ["k"], "why": "w", "origin": said});
                body.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
                id_of(&write(root, "x", "decision", body));
            }
            write(root, "x", "task", json!({"wave": 1, "text": "Criar o índice da tabela.", "files": [{"path": "src/a.rs"}],
                "depends_on": [], "covers": [done], "origin": said}));
        });
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.codes().into_iter().map(|(id, code)| (code, id)).collect()
    }

    /// O veredito final reprovado, com o combinado inteiro vigente
    /// atendido, gravado como o revisor o grava, com o pedido de revisão
    /// aberto antes: os testes daqui provam o mecanismo da escolha
    /// (`ANALYSIS`), não o do combinado — sem a lista `agreed` cobrindo todo
    /// mundo, a revisão final seria recusada por faltar item, antes de chegar
    /// à escolha que o teste quer provar. Devolve o relatório do revisor,
    /// vazio.
    fn verdict_with_agreed(root: &Path, wave: u64, result: &str, text: &str, agreed: &[&str]) -> String {
        let agreed: Vec<Value> = agreed.iter().map(|item| json!({"item": item, "met": true})).collect();
        seed_review(root);
        let wrote = judged(root, json!({"wave": wave, "result": result, "final": true, "text": text,
            "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}], "agreed": agreed}));
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        String::new()
    }

    /// A linha da escolha da onda 1: o que sai e o que entra, com o motivo.
    fn analysis(removed: Value, added: Value) -> String {
        line("ANALYSIS", json!({"wave": 1, "removed": removed, "added": added}))
    }

    /// Os códigos de um grupo de candidatos que a rodada entregou.
    fn codes_in(asked: &Value, group: &str) -> Vec<String> {
        let listed = asked[group].as_array().map(Vec::as_slice).unwrap_or_default();
        listed.iter().filter_map(|c| c["item"].as_str()).map(str::to_string).collect()
    }

    /// A rodada que vai soltar uma onda com regras do projeto todo, itens sem
    /// dono e lições que casam com ela entrega esses candidatos ao
    /// orquestrador, cada um com o título, e nenhum agente é pedido: a
    /// resposta não traz modelo nem pedido pronto, e o próximo passo não fala
    /// de Sonnet. A lição que não casa com a onda não é candidata. A escolha
    /// volta na linha que a rodada lê e fica gravada no envio com o motivo, a
    /// da lição inclusive: o pedido leva a lição que ficou e não a que saiu.
    #[test]
    fn the_round_hands_the_candidates_to_the_orchestrator() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let ids = with_items_to_judge(root);
        let lesson = |text: &str, key: &str| {
            id_of(&write(root, "x", "lesson", json!({"class": "environment_trap", "text": text, "keys": [key],
                "applies_to": {"files": ["**"]}})))
        };
        let kept = lesson("A tabela nova precisa de migração.", "tabela");
        let dropped = lesson("O índice novo deixa a busca lenta.", "índice");
        let far = lesson("O terminal do Windows troca a barra.", "terminal");

        let asked = round(root, "x", None);
        assert_eq!(asked["ok"], json!(true), "{asked}");
        assert_eq!(waves_in(&asked, "dispatch"), Vec::<u64>::new(), "{asked}");
        let title = |code: &str, title: &str| json!({"item": code, "title": title});
        assert_eq!(
            asked["analysis"],
            json!([{
                "wave": 1,
                "title": "Onda 1.",
                "project": [title("MSTD-RULE-0001", "Vale sempre: a tabela nova tem chave."),
                    title("MSTD-RULE-0002", "Vale sempre: a spec vira um PR só.")],
                "unowned": [title("MSTD-DEC-0001", "Sem dono: a tabela nasce vazia."),
                    title("MSTD-DEC-0002", "Sem dono: o download não muda.")],
                "lessons": [{"lesson": kept, "title": "A tabela nova precisa de migração."},
                    {"lesson": dropped, "title": "O índice novo deixa a busca lenta."}],
            }]),
            "the lesson {far} does not fit the wave: {asked}"
        );
        let next = asked["next"].as_str().unwrap_or_default();
        assert!(!next.to_lowercase().contains("sonnet") && !next.contains("model"), "{next}");
        assert!(next.contains("<ANALYSIS>"), "the next step teaches the line: {next}");

        let removed = json!([{"item": "MSTD-RULE-0002", "why": "Fala da entrega, e não da tabela."},
            {"lesson": dropped, "why": "A busca não muda nesta onda."}]);
        let added = json!([{"item": "MSTD-DEC-0001", "why": "A tabela nova nasce vazia."}]);
        let out = round(root, "x", Some(&analysis(removed, added)));
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert!(out.get("warnings").is_none(), "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "send").collect();
        assert_eq!(sent.len(), 1, "{out}");
        let judged: Vec<u64> = ["MSTD-RULE-0001", "MSTD-RULE-0002", "MSTD-DEC-0001", "MSTD-DEC-0002"].map(|c| ids[c]).to_vec();
        let recorded = json!({"judged": judged,
            "removed": [{"item": ids["MSTD-RULE-0002"], "why": "Fala da entrega, e não da tabela."}],
            "added": [{"item": ids["MSTD-DEC-0001"], "why": "A tabela nova nasce vazia."}],
            "judged_lessons": [kept, dropped],
            "removed_lessons": [{"lesson": dropped, "why": "A busca não muda nesta onda."}],
            "tasks": []});
        assert_eq!(sent[0].fields.get("analysis"), Some(&recorded), "the send records the choice: {out}");
        let prompt = out["dispatch"][0]["prompt"].as_str().unwrap_or_default();
        // Lições vão só pelo número, sem o texto delas no pedido.
        assert!(prompt.lines().any(|l| l == format!("- `lessons`: {kept}")), "{prompt}");
        for out_of_it in [dropped, far] {
            assert!(!prompt.contains(&format!("- `lessons`: {out_of_it}")), "{out_of_it}: {prompt}");
        }
        for out_of_it in ["A tabela nova precisa de migração.", "O índice novo deixa a busca lenta.", "O terminal do Windows troca a barra."] {
            assert!(!prompt.contains(out_of_it), "{out_of_it}: {prompt}");
        }
        assert!(prompt.contains(CHOSEN_ITEMS), "{prompt}");
    }

    /// A rodada que vai soltar uma onda com itens do projeto todo e itens sem
    /// dono entrega os candidatos ao orquestrador: nada sai, nenhum envio é
    /// gravado e nenhuma cópia é criada; sem a escolha, entrega de novo. Com
    /// a linha da escolha, a onda sai: o pedido e o envio levam o que ficou,
    /// sem o que saiu e com o que entrou, e o envio grava à parte o que saiu
    /// e o que entrou, com o motivo. O item que as tarefas da onda fazem vai
    /// sempre: ele não é candidato, e a linha que tenta tirá-lo fica sem
    /// efeito, como a que vem sem motivo. O gancho que monta o pedido no
    /// despacho chega ao mesmo texto. Na divisa do conserto: com os mesmos
    /// candidatos, o conserto sai com a escolha gravada; com uma regra nova do
    /// projeto todo, a rodada pede a escolha de novo.
    #[test]
    fn the_round_records_the_analysis_before_sending() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let ids = with_items_to_judge(root);
        let sends = || {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let sent = log.visible().into_iter().filter(|e| e.event_type == "send" && e.wave().is_some());
            sent.map(|e| e.fields.clone()).collect::<Vec<_>>()
        };

        // Sem a escolha, a onda não sai.
        for _ in 0..2 {
            let asked = round(root, "x", None);
            assert_eq!(asked["ok"], json!(true), "{asked}");
            assert_eq!(waves_in(&asked, "dispatch"), Vec::<u64>::new(), "{asked}");
            assert!(sends().is_empty(), "no send before the analysis: {asked}");
            assert!(!slot_path(root, "x", 0).exists(), "no copy before the analysis");
            let request = &asked["analysis"][0];
            assert_eq!(request["wave"], json!(1), "{asked}");
            assert_eq!(codes_in(request, "project"), ["MSTD-RULE-0001", "MSTD-RULE-0002"], "the rule the task does is not judged");
            assert_eq!(codes_in(request, "unowned"), ["MSTD-DEC-0001", "MSTD-DEC-0002"], "{asked}");
            assert_eq!(request["lessons"], json!([]), "{asked}");
            let next = translate("round.analysis", Locale::PtBr).replace("{waves}", "1");
            assert!(asked["next"].as_str().unwrap_or_default().contains(&next), "{asked}");
        }

        // Com a escolha, a onda sai com ela.
        let removed = json!([{"item": "MSTD-RULE-0002", "why": "Fala da entrega, e não da tabela."},
            {"item": "MSTD-RULE-0003", "why": "Tentou tirar a que a tarefa faz."}]);
        let added = json!([{"item": "MSTD-DEC-0001", "why": "A tabela nova nasce vazia."},
            {"item": "MSTD-DEC-0002", "why": ""}]);
        let out = round(root, "x", Some(&analysis(removed, added)));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert!(out.get("analysis").is_none(), "{out}");
        let prompt = out["dispatch"][0]["prompt"].as_str().unwrap_or_default().to_string();
        assert!(prompt.contains(CHOSEN_ITEMS), "{prompt}");
        let ignored: Vec<&str> = out["warnings"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|w| w["reason"] == json!("analysis-item-ignored"))
            .filter_map(|w| w["hint"].as_str())
            .collect();
        assert_eq!(ignored.len(), 2, "{out}");
        assert!(ignored[0].contains("MSTD-RULE-0003") && ignored[1].contains("MSTD-DEC-0002"), "{ignored:?}");
        let sent = sends();
        assert_eq!(sent.len(), 1, "{out}");
        assert_eq!(sent[0]["text"], json!(prompt));
        let kept: Vec<u64> = ["MSTD-RULE-0001", "MSTD-RULE-0003", "MSTD-DEC-0001", "MSTD-DEC-0003"].map(|c| ids[c]).to_vec();
        let items: Vec<u64> = sent[0]["items"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
        for code in ["MSTD-RULE-0002", "MSTD-DEC-0002"] {
            assert!(!items.contains(&ids[code]), "{code} stayed out: {items:?}");
        }
        assert!(kept.iter().all(|id| items.contains(id)), "{items:?}");
        let judged: Vec<u64> = ["MSTD-RULE-0001", "MSTD-RULE-0002", "MSTD-DEC-0001", "MSTD-DEC-0002"].map(|c| ids[c]).to_vec();
        assert_eq!(sent[0]["analysis"], json!({"judged": judged,
            "removed": [{"item": ids["MSTD-RULE-0002"], "why": "Fala da entrega, e não da tabela."}],
            "added": [{"item": ids["MSTD-DEC-0001"], "why": "A tabela nova nasce vazia."}],
            "judged_lessons": [], "removed_lessons": [], "tasks": []}));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let running = waves_in_progress(&log).into_keys().collect();
        let flight = mustard_core::io::wave_prompt::Flight { running, ..Default::default() };
        let hooked = mustard_core::io::wave_prompt::prompts(root, "x", &log, Locale::PtBr, &flight);
        assert_eq!(hooked.iter().find(|p| p.wave == 1).map(|p| p.text.as_str()), Some(prompt.as_str()), "the hook builds the same request");

        // O conserto, com os mesmos itens a julgar, sai com a escolha gravada.
        round(root, "x", Some(&delivered(root, 1, "A tabela saiu.", &["src/a.rs"])));
        let vigent_before = ["MSTD-RULE-0001", "MSTD-RULE-0002", "MSTD-RULE-0003",
            "MSTD-DEC-0001", "MSTD-DEC-0002", "MSTD-DEC-0003"];
        let fix = round(root, "x", Some(&verdict_with_agreed(root, 1, "rejected", "faltou o índice", &vigent_before)));
        assert_eq!(waves_in(&fix, "dispatch"), vec![1], "{fix}");
        let sent = sends();
        assert_eq!(sent.len(), 2, "{fix}");
        assert_eq!(sent[1]["analysis"], sent[0]["analysis"], "{fix}");

        // Com uma regra nova do projeto todo, o conserto seguinte pede a
        // análise de novo, e sai com a escolha nova.
        round(root, "x", Some(&delivered(root, 1, "O índice entrou.", &["src/a.rs"])));
        let said = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
            .visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
        let newer = id_of(&write(root, "x", "rule", json!({"text": "Vale sempre: o nome é curto.", "example": "e",
            "keys": ["k"], "applies_to": {"files": ["**"]}, "origin": said})));
        let vigent_with_newer = ["MSTD-RULE-0001", "MSTD-RULE-0002", "MSTD-RULE-0003", "MSTD-RULE-0004",
            "MSTD-DEC-0001", "MSTD-DEC-0002", "MSTD-DEC-0003"];
        let again = round(root, "x", Some(&verdict_with_agreed(root, 1, "rejected", "faltou o nome", &vigent_with_newer)));
        assert_eq!(waves_in(&again, "dispatch"), Vec::<u64>::new(), "{again}");
        assert_eq!(codes_in(&again["analysis"][0], "project"), ["MSTD-RULE-0001", "MSTD-RULE-0002", "MSTD-RULE-0004"], "{again}");
        assert_eq!(sends().len(), 2, "{again}");
        let out = round(root, "x", Some(&analysis(json!([]), json!([]))));
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let sent = sends();
        assert!(sent[2]["analysis"]["judged"].as_array().unwrap().contains(&json!(newer)), "{out}");
        assert_eq!(sent[2]["analysis"]["removed"], json!([]), "{out}");
    }

    /// A lição nova, sozinha, faz o conserto pedir a escolha de novo: com os
    /// mesmos itens do projeto todo e sem dono, o conserto reusa a escolha
    /// gravada; mas uma lição do banco que passa a casar com a onda — sem
    /// nenhum item novo do projeto — já basta para a escolha gravada não
    /// cobrir mais os candidatos, e o conserto volta a perguntar antes de
    /// sair.
    #[test]
    fn a_new_lesson_alone_makes_the_fix_ask_for_the_choice_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        with_items_to_judge(root);
        let kept = id_of(&write(root, "x", "lesson", json!({"class": "environment_trap",
            "text": "A tabela nova precisa de migração.", "keys": ["tabela"], "applies_to": {"files": ["**"]}})));
        let sends = || {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let sent = log.visible().into_iter().filter(|e| e.event_type == "send" && e.wave().is_some());
            sent.map(|e| e.fields.clone()).collect::<Vec<_>>()
        };

        // A primeira rodada pede a escolha; com ela, a onda sai.
        let asked = round(root, "x", None);
        assert_eq!(waves_in(&asked, "dispatch"), Vec::<u64>::new(), "{asked}");
        assert_eq!(asked["analysis"][0]["lessons"], json!([{"lesson": kept, "title": "A tabela nova precisa de migração."}]), "{asked}");
        let out = round(root, "x", Some(&analysis(json!([]), json!([]))));
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert_eq!(sends().len(), 1, "{out}");

        // O conserto, com os mesmos candidatos — a mesma lição, nenhuma nova
        // —, reusa a escolha gravada, sem perguntar de novo.
        round(root, "x", Some(&delivered(root, 1, "A tabela saiu.", &["src/a.rs"])));
        let vigent = ["MSTD-RULE-0001", "MSTD-RULE-0002", "MSTD-RULE-0003",
            "MSTD-DEC-0001", "MSTD-DEC-0002", "MSTD-DEC-0003"];
        let fix = round(root, "x", Some(&verdict_with_agreed(root, 1, "rejected", "faltou algo", &vigent)));
        assert_eq!(waves_in(&fix, "dispatch"), vec![1], "o conserto com os mesmos candidatos reusa a escolha: {fix}");
        let sent = sends();
        assert_eq!(sent.len(), 2, "{fix}");
        assert_eq!(sent[1]["analysis"], sent[0]["analysis"], "{fix}");

        // Uma lição nova passa a casar com a onda — nenhum item do projeto
        // mudou. O conserto seguinte não reusa a escolha: ele pergunta de
        // novo, com a lição nova entre os candidatos.
        let newer = id_of(&write(root, "x", "lesson", json!({"class": "environment_trap",
            "text": "O índice novo evita busca lenta.", "keys": ["índice"], "applies_to": {"files": ["**"]}})));
        round(root, "x", Some(&delivered(root, 1, "O índice entrou.", &["src/a.rs"])));
        let again = round(root, "x", Some(&verdict_with_agreed(root, 1, "rejected", "faltou o nome", &vigent)));
        assert_eq!(waves_in(&again, "dispatch"), Vec::<u64>::new(), "a lição nova pede a escolha de novo: {again}");
        let lessons: Vec<u64> = again["analysis"][0]["lessons"].as_array().unwrap_or(&Vec::new())
            .iter().filter_map(|l| l["lesson"].as_u64()).collect();
        assert!(lessons.contains(&kept) && lessons.contains(&newer), "{again}");
        assert_eq!(sends().len(), 2, "sem escolha, nenhum envio novo sai: {again}");
    }

    /// A lição posta em `added` na linha da escolha vira aviso e nunca entra
    /// no pedido por essa via: a lição que casa com a onda já vai por conta
    /// própria, e pedir para "acrescentar" ela não a soma de novo nem some
    /// nenhum item sem dono que a mesma linha acrescente de verdade: o item
    /// entra no pedido, e a lição sai só como aviso.
    #[test]
    fn a_lesson_put_in_added_becomes_a_warning() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let ids = with_items_to_judge(root);
        let kept = id_of(&write(root, "x", "lesson", json!({"class": "environment_trap",
            "text": "A tabela nova precisa de migração.", "keys": ["tabela"], "applies_to": {"files": ["**"]}})));

        let removed = json!([]);
        let added = json!([{"item": "MSTD-DEC-0001", "why": "A tabela nova nasce vazia."}, {"lesson": kept, "why": "Vale para esta onda também."}]);
        let out = round(root, "x", Some(&analysis(removed, added)));
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let ignored: Vec<&str> = out["warnings"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|w| w["reason"] == json!("analysis-item-ignored"))
            .filter_map(|w| w["hint"].as_str())
            .collect();
        assert_eq!(ignored.len(), 1, "só a lição em `added` vira aviso: {out}");
        assert!(ignored[0].contains(&kept.to_string()) && ignored[0].contains("lição"), "{ignored:?}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send").unwrap();
        let analysis_field = sent.fields.get("analysis").cloned().unwrap_or_default();
        // O item sem dono, de verdade acrescentado, entra; a lição, que só
        // vai pelo caminho automático, não aparece em `added`.
        assert_eq!(analysis_field["added"], json!([{"item": ids["MSTD-DEC-0001"], "why": "A tabela nova nasce vazia."}]),
            "a lição não entra por `added`: {analysis_field}");
        // Ela segue candidata, e vai pelo caminho automático — não por ter
        // sido posta em `added`.
        assert_eq!(analysis_field["judged_lessons"], json!([kept]), "{analysis_field}");
        let prompt = out["dispatch"][0]["prompt"].as_str().unwrap_or_default();
        assert!(prompt.lines().any(|l| l == format!("- `lessons`: {kept}")), "{prompt}");
    }

    /// Duas rodadas ao mesmo tempo, com a mesma linha da análise, leem a spec
    /// antes de qualquer uma pegar a trava. Uma solta a onda com a escolha, e
    /// a outra lê a spec, com a trava presa, depois do envio dela e não a
    /// solta de novo: a onda tem um envio só, com a escolha.
    #[test]
    fn two_rounds_with_the_same_analysis_send_the_wave_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        with_items_to_judge(root);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), Vec::<u64>::new());
        let report = analysis(json!([{"item": "MSTD-RULE-0002", "why": "Fala da entrega."}]), json!([]));

        let outs = two_rounds_at_once(root, Some(&report));
        let dispatched: Vec<u64> = outs.iter().flat_map(|out| waves_in(out, "dispatch")).collect();
        assert_eq!(dispatched, vec![1], "only one round sends the wave out: {outs:?}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "send").collect();
        assert_eq!(sent.len(), 1, "{outs:?}");
        assert_eq!(sent[0].fields["analysis"]["removed"][0]["why"], json!("Fala da entrega."), "{outs:?}");
        for out in &outs {
            assert!(out.get("analysis").is_none(), "neither round asks again: {out}");
        }
    }

    /// A ferramenta do scan de mentira: relê o disco inteiro, fora `.claude`
    /// e `.git`, e grava o mapa no formato que a leitura de verdade espera.
    /// Como o mapa relê depois de cada commit da rodada, o arquivo que um
    /// commit acabou de apagar nunca aparece nele.
    fn rescan_disk(root: &Path, model: &Path) -> mustard_core::platform::error::Result<mustard_core::domain::scan::ScanReport> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<Value>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
            entries.sort();
            for path in entries {
                if path.file_name().is_some_and(|name| name == ".claude" || name == ".git") {
                    continue;
                }
                if path.is_dir() {
                    walk(root, &path, out);
                } else if let Ok(rel) = path.strip_prefix(root) {
                    out.push(json!({ "path": rel.to_string_lossy().replace('\\', "/") }));
                }
            }
        }
        let mut modules = Vec::new();
        walk(root, root, &mut modules);
        mustard_core::io::project_map::write_text_at(model, &json!({ "modules": modules }).to_string()).unwrap();
        Ok(mustard_core::domain::scan::ScanReport { full: true, files: modules.len(), ..Default::default() })
    }

    /// A rodada vai soltar a onda 2, cuja tarefa casa com a skill `calculadora`
    /// do projeto e tem um arquivo parecido no mapa, logo depois de um commit
    /// que apagou outro arquivo parecido: a resposta mostra ao orquestrador,
    /// por tarefa, a skill sugerida e até três arquivos parecidos, nenhum
    /// deles apagado; a escolha dele vai gravada no envio, e o pedido leva a
    /// skill e os arquivos de leitura.
    #[test]
    fn the_round_suggests_skills_and_examples_per_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved_with(root, "x", &[(1, &["src/calculadora_velha.rs"], &[])], |said| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap().id;
            write(
                root,
                "x",
                "wave",
                json!({"n": 2, "text": "Onda 2.", "criteria": [crit], "done_when": "A suíte passa.",
                    "depends_on": [1], "origin": said}),
            );
            write(
                root,
                "x",
                "task",
                json!({"wave": 2, "text": "Montar uma calculadora nova.",
                    "files": [{"path": "src/calculadora_nova.rs"}], "depends_on": [], "origin": said}),
            );
        });
        std::fs::write(root.join("src/calculadora_nova.rs"), "fn nova() {}\n").unwrap();
        let skill = root.join(".claude/skills/calculadora");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: calculadora\ndescription: Use quando for preciso montar uma calculadora.\n---\n\nMonte.\n",
        )
        .unwrap();

        // A onda 1 sai; a 2 depende dela e ainda não está pronta.
        let first = round_with_mine(root, "x", None, &rescan_disk);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");

        // A onda 1 entrega apagando o arquivo parecido antigo: o mapa relê
        // antes de a onda 2 pedir a escolha, na mesma volta.
        std::fs::remove_file(root.join("src/calculadora_velha.rs")).unwrap();
        let back = json!({"wave": 1, "text": "Saiu.", "files": ["src/calculadora_velha.rs"],
            "commit": "tira a calculadora velha"});
        assert_eq!(returned(root, back)["ok"], json!(true));
        let asked = round_with_mine(root, "x", None, &rescan_disk);
        assert_eq!(asked["ok"], json!(true), "{asked}");
        assert_eq!(waves_in(&asked, "dispatch"), Vec::<u64>::new(), "{asked}");
        let tasks = asked["analysis"][0]["tasks"].as_array().cloned().unwrap_or_default();
        assert_eq!(tasks.len(), 1, "{asked}");
        assert_eq!(tasks[0]["matched_skills"], json!(["calculadora"]), "{asked}");
        let files: Vec<String> =
            tasks[0]["files"].as_array().unwrap().iter().filter_map(|f| f.as_str()).map(str::to_string).collect();
        assert!(files.iter().any(|f| f.contains("calculadora_nova.rs")), "{files:?}");
        assert!(
            !files.iter().any(|f| f.contains("calculadora_velha.rs")),
            "the deleted file is never suggested: {files:?}"
        );
        let task_code = tasks[0]["task"].as_str().unwrap_or_default().to_string();

        // O orquestrador confirma a skill e o arquivo: a escolha vai gravada
        // no envio, e o pedido leva os dois.
        let chosen = line(
            "ANALYSIS",
            json!({"wave": 2, "removed": [], "added": [],
                "tasks": [{"task": task_code, "skills": ["calculadora"], "files": ["src/calculadora_nova.rs"]}]}),
        );
        let out = round_with_mine(root, "x", Some(&chosen), &rescan_disk);
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "{out}");
        let prompt = out["dispatch"][0]["prompt"].as_str().unwrap_or_default();
        assert!(prompt.contains("**calculadora**"), "{prompt}");
        assert!(prompt.contains("src/calculadora_nova.rs"), "{prompt}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().rfind(|e| e.event_type == "send" && e.wave() == Some(2)).unwrap();
        let codes = log.codes();
        let task_id = codes.iter().find(|(_, code)| **code == task_code).map(|(id, _)| *id).unwrap();
        assert_eq!(
            sent.fields["analysis"]["tasks"],
            json!([{"task": task_id, "skills": ["calculadora"], "files": ["src/calculadora_nova.rs"]}]),
            "the choice is recorded on the send"
        );
    }

    /// O mapa do projeto acompanha o commit atual antes de montar o pedido:
    /// enquanto o commit e a listagem do git gravados no mapa batem com os do
    /// checkout, a ferramenta do scan não roda de novo; um commit feito fora
    /// da rodada —
    /// à mão, ou um pull — muda as linhas de uma função, e o pedido seguinte
    /// sai com as linhas novas, sem que a própria rodada precise de um
    /// commit dela para reler o mapa. A onda fica retida por um teto de
    /// compilações zerado na primeira volta, só para a comparação do mapa
    /// rodar sem nenhuma onda pronta para despachar; a segunda volta libera o
    /// teto, e é o pedido dela que mostra as linhas.
    #[test]
    fn the_map_follows_the_code_before_each_request() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn antes() {}\n\nfn soma() {\n    1 + 1;\n}\n").unwrap();
        approved_with(root, "x", &[], |said| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap().id;
            write(root, "x", "wave", json!({"n": 1, "text": "Onda 1.", "criteria": [crit],
                "done_when": "A suíte passa.", "origin": said}));
            write(root, "x", "task", json!({"wave": 1, "text": "Somar.",
                "files": [{"path": "src/a.rs"}], "depends_on": [], "must_read": ["src/a.rs#soma"], "origin": said}));
        });
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":0}"#).unwrap();
        // O que a instalação grava no projeto fica fora do git: a spec que a
        // rodada escreve não muda a listagem.
        std::fs::write(root.join(".git/info/exclude"), mustard_core::footprint_rules().join("\n") + "\n").unwrap();

        // O mapa já foi lido no commit atual (a "semente"), com a função em
        // 3-5, e guarda a listagem do git de agora.
        let head_v1 = git_text(root, &["rev-parse", "HEAD"]);
        let listing = mustard_core::io::project_map::listing(root).expect("o projeto está no git").digest();
        mustard_core::io::project_map::write_text(
            root,
            &json!({
                "modules": [{"path": "src/a.rs",
                    "declarations": [{"kind": "function", "name": "soma", "line": 3, "end_line": 5}]}],
                "state": {"head": head_v1, "listing": listing},
            })
            .to_string(),
        )
        .unwrap();

        // O teto está em zero: nenhuma onda sai, mas o mapa já é conferido.
        // Nada mudou: a ferramenta do scan não roda.
        fn mine_untouched(
            _: &Path,
            _: &Path,
        ) -> mustard_core::platform::error::Result<mustard_core::domain::scan::ScanReport> {
            panic!("a ferramenta do scan não deveria rodar com o mapa em dia");
        }
        let first = round_with_mine(root, "x", None, &mine_untouched);
        assert_eq!(waves_in(&first, "dispatch"), Vec::<u64>::new(), "{first}");

        // Um commit feito fora da rodada, sem passar pelo commit dela.
        git_at(root, &["commit", "--allow-empty", "-q", "-m", "fora da rodada"]);
        let head_v2 = git_text(root, &["rev-parse", "HEAD"]);
        assert_ne!(head_v1, head_v2, "o commit avançou");

        let mine_refreshed = move |_: &Path, out: &Path| {
            mustard_core::io::project_map::write_text_at(
                out,
                &json!({
                    "modules": [{"path": "src/a.rs",
                        "declarations": [{"kind": "function", "name": "soma", "line": 13, "end_line": 15}]}],
                    "state": {"head": head_v2},
                })
                .to_string(),
            )
            .unwrap();
            Ok(mustard_core::domain::scan::ScanReport { full: false, files: 1, head: head_v2.clone(), ..Default::default() })
        };

        // O teto libera a vaga, sem nenhum commit da própria rodada: só o
        // mapa desatualizado explica a releitura a seguir.
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();

        // Com a busca casando "somar" a um item da spec, a rodada pede a
        // escolha antes de soltar a onda: nada despacha ainda.
        let second = round_with_mine(root, "x", None, &mine_refreshed);
        assert_eq!(waves_in(&second, "dispatch"), Vec::<u64>::new(), "{second}");
        assert!(second.get("analysis").is_some(), "{second}");

        // Com a linha da escolha, a onda sai, e o pedido segue o mapa do
        // commit atual.
        let third = round_with_mine(root, "x", Some(&analysis(json!([]), json!([]))), &mine_refreshed);
        assert_eq!(waves_in(&third, "dispatch"), vec![1], "{third}");
        let prompt = third["dispatch"][0]["prompt"].as_str().unwrap_or_default();
        assert!(
            prompt.contains("leia só as linhas 13-15 de `soma` em `src/a.rs`"),
            "o pedido segue o commit atual: {prompt}"
        );
        assert!(!prompt.contains("3-5"), "{prompt}");
    }

    /// A resposta da rodada mostra o estado de cada onda em andamento, para o
    /// orquestrador responder "como estamos?" sem conferir a cópia à mão: os
    /// minutos desde o envio, os arquivos que a cópia já tem mudados, o
    /// código e o texto do último passo gravado depois do envio, e os
    /// minutos desde o último sinal de vida. A onda sem passo gravado mostra
    /// só o que tem, sem a chave `step`.
    #[test]
    fn the_round_shows_the_state_of_each_running_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();

        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");

        // A onda 1 ganha um arquivo novo, ainda não entregue, na cópia dela;
        // e um passo gravado depois do envio.
        let copy1 = sent_copy(root, 1);
        std::fs::write(Path::new(&copy1).join("rascunho.txt"), "x").unwrap();
        write(root, "x", "step", json!({"wave": 1, "item": "MSTD-TASK-0001", "text": "A tarefa 1 ficou pronta."}));

        let second = round(root, "x", None);
        let running = second["running"].as_array().cloned().unwrap_or_default();
        let entry_of = |wave: u64| running.iter().find(|r| r["wave"] == json!(wave)).cloned().unwrap_or_else(|| panic!("wave {wave}: {running:?}"));

        let one = entry_of(1);
        assert!(one["send"].as_str().is_some(), "{one}");
        assert!(one["minutes"].as_i64().is_some(), "os minutos desde o envio: {one}");
        assert_eq!(one["files"], json!(["rascunho.txt"]), "{one}");
        assert_eq!(one["step"], json!({"item": "MSTD-TASK-0001", "text": "A tarefa 1 ficou pronta."}), "{one}");
        assert!(one["silent_minutes"].as_i64().is_some(), "os minutos desde o sinal de vida: {one}");

        // A onda 2 não ganhou passo nenhum: a chave `step` fica de fora.
        let two = entry_of(2);
        assert!(two.get("step").is_none(), "sem passo, a chave fica de fora: {two}");
        assert_eq!(two["files"], json!([]), "a cópia da onda 2 não mudou: {two}");
        assert!(two["minutes"].as_i64().is_some(), "{two}");
        assert!(two["silent_minutes"].as_i64().is_some(), "{two}");

        // Nenhum aviso novo acorda o orquestrador: sem lista de "stuck" nem
        // pergunta, e a rodada não passa do que já se pergunta hoje.
        assert!(second.get("stopped").is_none(), "{second}");
    }

    /// O silêncio de uma onda conta da ação mais nova: o sinal de vida da
    /// vaga que o envio gravou, quando é mais novo que o envio; a hora do
    /// envio, quando o sinal da vaga ficou de uma onda anterior.
    #[test]
    fn the_silence_of_a_wave_counts_from_the_newest_between_its_slot_signal_and_its_send() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let copy = sent_copy(root, 1);
        let slot = Path::new(&copy).file_name().unwrap().to_string_lossy().into_owned();
        let alive = crate::hooks::observe::wave_alive_observer::alive_path(root, "x", &slot);
        std::fs::create_dir_all(alive.parent().unwrap()).unwrap();
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.last_by_wave("send")[&1];
        let at = |minutes: i64| (chrono::Local::now() + chrono::Duration::minutes(minutes)).to_rfc3339();

        std::fs::write(&alive, at(30)).unwrap();
        let newer = silent_minutes(root, "x", 1, &log, sent).unwrap();
        assert!(newer <= -29, "o sinal da vaga, mais novo que o envio, vale: {newer}");

        std::fs::write(&alive, at(-120)).unwrap();
        let older = silent_minutes(root, "x", 1, &log, sent).unwrap();
        assert!((0..=1).contains(&older), "o sinal de uma onda anterior não conta: {older}");
    }

    /// Um arquivo já rastreado, mudado sem estar preparado, sai do `git
    /// status` com o código de estado começando em espaço (`" M arquivo"`);
    /// a saída inteira é trimada antes de virar linhas, o que apaga esse
    /// espaço quando o arquivo é o primeiro. A lista de arquivos mudados
    /// continua trazendo o nome inteiro, sem a primeira letra cortada.
    #[test]
    fn the_wave_is_found_by_the_paths_of_the_call() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);

        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");

        let copy1 = sent_copy(root, 1);
        std::fs::write(Path::new(&copy1).join("src").join("a.rs"), "fn dois() {}\n").unwrap();

        let second = round(root, "x", None);
        let running = second["running"].as_array().cloned().unwrap_or_default();
        let one = running.iter().find(|r| r["wave"] == json!(1)).cloned().unwrap_or_else(|| panic!("wave 1: {running:?}"));
        assert_eq!(one["files"], json!(["src/a.rs"]), "{one}");
    }

    /// A ordem de despacho sai do grafo de dependências que cada onda
    /// declara, com desempate pela quantidade de ondas que cada uma destrava
    /// — direta ou por outra —, da maior para a menor, e só depois pelo
    /// número: a onda 21 destrava a 22 e a 20 não destrava nada, então a 21
    /// sai primeiro mesmo tendo o número maior. O mesmo conjunto de ondas, aprovada duas
    /// vezes com as ondas declaradas em ordem diferente, despacha sempre na
    /// mesma sequência através das rodadas — a prova atravessa `round`, o
    /// comando de verdade, não a função auxiliar que só ordena.
    #[test]
    fn o_mesmo_conjunto_de_ondas_produz_sempre_a_mesma_ordem_de_despacho() {
        let plan_a: [(u64, &[&str], &[u64]); 4] = [
            (10, &["src/a.rs"], &[]),
            (20, &["src/b.rs"], &[10]),
            (21, &["src/c.rs"], &[10]),
            (22, &["src/d.rs"], &[21]),
        ];
        let plan_b: [(u64, &[&str], &[u64]); 4] = [
            (22, &["src/d.rs"], &[21]),
            (21, &["src/c.rs"], &[10]),
            (20, &["src/b.rs"], &[10]),
            (10, &["src/a.rs"], &[]),
        ];
        let mut sequences = Vec::new();
        for plan in [plan_a.as_slice(), plan_b.as_slice()] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", plan);
            let first = round(root, "x", None);
            let second = round(root, "x", Some(&delivered(root, 10, "Saiu.", &["src/a.rs"])));
            let third = round(
                root,
                "x",
                Some(&format!(
                    "{}\n{}",
                    delivered(root, 21, "Saiu.", &["src/c.rs"]),
                    delivered(root, 20, "Saiu.", &["src/b.rs"])
                )),
            );
            sequences.push(vec![waves_in(&first, "dispatch"), waves_in(&second, "dispatch"), waves_in(&third, "dispatch")]);
        }
        let expected = vec![vec![10], vec![21, 20], vec![22]];
        assert_eq!(sequences[0], expected, "o backlog declarada numa ordem: {sequences:?}");
        assert_eq!(sequences[1], expected, "o mesmo conjunto de ondas, declarada noutra ordem, despacha igual: {sequences:?}");
    }

    /// Ondas que dependem umas das outras em círculo são recusadas pela
    /// própria rodada, mostrando o ciclo: nenhuma sai, e a resposta não é uma
    /// fila vazia muda. O backlog nunca passa pela aprovação do plano — o
    /// fixture grava o ciclo direto, como um evento gravado à mão poderia —
    /// e a prova atravessa `round`, o comando de verdade.
    #[test]
    fn conjunto_de_ondas_com_dependencia_em_circulo_e_recusado_pela_rodada_mostrando_o_ciclo() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(4, &["src/a.rs"], &[7]), (7, &["src/b.rs"], &[4])]);
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("waves-loop"), "{out}");
        assert_eq!(out["hint"], json!(WAVE_LOOP_4_7), "a recusa nomeia as ondas do ciclo: {out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "nenhuma onda sai com o ciclo: {out}");
        // Sem relatório, a rodada não gravou entrega nem comitou nada antes
        // de achar o ciclo: a resposta não inventa gravação.
        assert_eq!(out["recorded"], json!([]), "{out}");
        assert!(out.get("commit").is_none(), "{out}");
    }

    /// A recusa das ondas 4 e 7, que dependem uma da outra, como a pessoa a
    /// lê: as duas pelo número, e nenhuma outra.
    const WAVE_LOOP_4_7: &str =
        "As ondas 4, 7 dependem umas das outras em círculo, e nenhuma pode começar. Corte uma das dependências.";

    /// A rodada que recebe a entrega da onda 1 junta, comita e grava a
    /// entrega, e só depois, ao escolher as ondas seguintes, acha as ondas 4 e
    /// 7 em círculo. A recusa não troca a resposta: ela vem junto do que foi
    /// gravado, do commit e das instruções de cópia da página, e nomeia só as
    /// ondas do ciclo — a 1, entregue, fica fora dela. A prova atravessa
    /// `round`, o comando de verdade.
    #[test]
    fn a_recusa_de_ciclo_devolve_o_que_a_rodada_gravou() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);

        // O ciclo entra depois do despacho da onda 1, gravado direto, como um
        // evento gravado à mão poderia: a rodada seguinte só o encontra
        // depois de juntar a entrega.
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let visible = log.visible();
        let said = visible.iter().find(|e| e.event_type == "message").unwrap().id;
        let crit = visible.iter().find(|e| e.event_type == "criterion").unwrap().id;
        for (n, on, file) in [(4u64, 7u64, "src/b.rs"), (7, 4, "src/c.rs")] {
            write(root, "x", "wave", json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit],
                "done_when": "A suíte passa.", "depends_on": [on], "origin": said}));
            write(root, "x", "task", json!({"wave": n, "text": format!("Tarefa da onda {n}."),
                "files": [{"path": file}], "depends_on": [], "origin": said}));
        }

        let out = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("waves-loop"), "{out}");
        assert_eq!(out["hint"], json!(WAVE_LOOP_4_7), "a recusa nomeia as ondas do ciclo, e só elas: {out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "nenhuma onda sai com o ciclo: {out}");

        // O que foi gravado: a entrega da onda 1, que está no arquivo de
        // eventos com o número que a resposta traz.
        let recorded = out["recorded"].as_array().cloned().unwrap_or_default();
        let entry = recorded
            .iter()
            .find(|r| r["type"] == json!("delivered") && r["wave"] == json!(1))
            .unwrap_or_else(|| panic!("a entrega da onda 1 na resposta: {out}"));
        let log = store::read(&path).unwrap().unwrap();
        let written = log.get(entry["id"].as_u64().unwrap_or_default()).unwrap_or_else(|| panic!("{out}"));
        assert_eq!(written.event_type, "delivered", "{out}");

        // O commit da rodada: na resposta e no git.
        assert!(out.get("commit").is_some_and(|c| !c.is_null()), "o commit vem na resposta: {out}");
        let subject = Command::new("git").args(["log", "-1", "--format=%s"]).current_dir(root).output().unwrap();
        assert!(String::from_utf8_lossy(&subject.stdout).contains("a onda 1 saiu"), "{out}");

        // As instruções de cópia da página, com a recusa como o passo seguinte.
        let next = out["next"].as_str().unwrap_or_default();
        assert!(next.contains("Leia `.claude/spec/x/copy/next.md`"), "{next}");
        assert!(next.ends_with(WAVE_LOOP_4_7), "{next}");
        assert!(out["copy"].is_object(), "{out}");
        let order = std::fs::read_to_string(root.join(".claude/spec/x/copy/next.md")).expect("a ordem de cópia");
        assert!(order.contains("ArtifactData"), "{order}");
    }

    /// A tarefa que sai de um lote de duas por evento de remoção, depois de o
    /// lote já ter sido formado, não deixa rastro no pedido dele: nem na
    /// lista de tarefas, nem no parágrafo de abertura que abre pelo
    /// `done_when` — que, sem critério com prova (o caso do item combinado
    /// sem dono, que não tem prova), é o texto das próprias tarefas unido
    /// por espaço, o caminho pelo qual o texto de uma tarefa retirada podia
    /// vazar no pedido. A prova atravessa `round`, o comando de verdade,
    /// para exercitar o pedido como o agente o recebe.
    #[test]
    fn o_pedido_da_onda_nao_cita_tarefa_retirada() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let said = log.visible().into_iter().find(|e| e.event_type == "message").unwrap().id;

        // Dois itens combinados sem dono (uma decisão cada), sem prova — o
        // caso que o backlog empacota quando o levantamento marca um item sem
        // onda: `waves` os dá dono antes mesmo de a onda 2 existir.
        let item1 = id_of(&write(
            root,
            "x",
            "decision",
            json!({"text": "Um item.", "keys": ["k1"], "why": "porque sim", "waves": [2], "origin": said}),
        ));
        let item2 = id_of(&write(
            root,
            "x",
            "decision",
            json!({"text": "Outro item.", "keys": ["k2"], "why": "porque sim", "waves": [2], "origin": said}),
        ));

        // Duas tarefas soltas, sem arquivo em comum, cada uma cobrindo um
        // item sem prova: o pronto-quando do lote cai nos títulos delas — o
        // mesmo caminho pelo qual o texto da tarefa retirada chegaria ao
        // pedido.
        let t1 = id_of(&write(
            root,
            "x",
            "task",
            json!({"title": "Gravar a versao nova de uma decisao", "text": "A decisao ja foi feita fora da onda.",
                "files": [{"path": "src/b.rs"}], "depends_on": [], "covers": [item1], "origin": said}),
        ));
        id_of(&write(
            root,
            "x",
            "task",
            json!({"title": "Trocar a mensagem de erro do campo vazio", "text": "Hoje a mensagem nao diz o campo.",
                "files": [{"path": "src/c.rs"}], "depends_on": [], "covers": [item2], "origin": said}),
        ));

        let log = store::read(&path).unwrap().unwrap();
        let formed = dispatch_backlog(root, "x", &log, &log).expect("formou o lote");
        assert_eq!(formed, vec![2], "as duas tarefas soltas viram um lote só: {formed:?}");

        // A tarefa 1 sai do backlog por remoção, depois de o lote já ter sido
        // formado: o trabalho dela já foi feito fora da onda. A gravação do
        // lote deu a ela uma versão nova, com o número da onda; a remoção a
        // aponta pelo código, que tira todas as versões, como o número de
        // qualquer versão da tarefa também tira.
        let log = store::read(&path).unwrap().unwrap();
        let t1_code = log.codes().get(&t1).cloned().expect("a tarefa 1 tem código");
        write(root, "x", "remove", json!({"targets": [t1_code], "reason": "o trabalho ja foi feito fora da onda"}));

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1, 2], "{out}");
        let dispatched = out["dispatch"].as_array().cloned().unwrap_or_default();
        let entry = dispatched.iter().find(|d| d["wave"] == json!(2)).unwrap_or_else(|| panic!("wave 2: {out}"));
        let prompt = entry["prompt"].as_str().unwrap_or_default();
        assert!(
            !prompt.contains("Gravar a versao nova de uma decisao"),
            "o texto da tarefa retirada nao pode aparecer no pedido: {prompt}"
        );
        assert!(
            prompt.contains("Trocar a mensagem de erro do campo vazio\n"),
            "o pronto-quando nasce das tarefas visiveis agora: {prompt}"
        );
        for whole in ["A decisao ja foi feita", "Hoje a mensagem nao diz"] {
            assert!(!prompt.contains(whole), "o texto inteiro da tarefa nao entra no pedido: {prompt}");
        }
    }

    /// Os motivos dos avisos do plano que conferiam onda como desenho: ondas
    /// da mesma rodada dividindo arquivo, onda ou spec que podia sair
    /// dividida, e tarefa cujo texto não casa com o da onda.
    const WAVE_DRAWING_REASONS: [&str; 5] = [
        "waves-share-a-file",
        "wave-should-split",
        "spec-should-split",
        "task-in-the-wrong-wave",
        "task-matches-no-wave",
    ];

    /// Um projeto com `src/a.rs` e `src/b.rs` no git e uma spec aprovada sem
    /// onda nenhuma: as tarefas vão para o backlog. Devolve o número da fala
    /// do usuário e o do critério, que as tarefas citam.
    fn backlog_project(root: &Path) -> (u64, u64) {
        backlog_project_with(root, |_| {})
    }

    /// [`backlog_project`] com o que o teste grava antes da aprovação
    /// (`before`), que recebe o número da fala do usuário.
    fn backlog_project_with(root: &Path, before: impl FnOnce(u64)) -> (u64, u64) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["a.rs", "b.rs"] {
            std::fs::write(root.join("src").join(name), "fn um() {}\n").unwrap();
        }
        approved_with(root, "x", &[], |said| {
            // O levantamento fechado, ponto a ponto, como o plano exige.
            let goal = "Mexer no código de um e de dois.";
            id_of(&write(root, "x", "context", json!({"text": goal, "origin": said})));
            let opts = crate::commands::flow::grill::GrillOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                kinds: Some("fix".into()),
                condensed: false,
            };
            let listed = crate::commands::flow::grill::grill_for(&opts, None);
            for item in listed["points"].as_array().cloned().unwrap_or_default() {
                let mut point = item.clone();
                point["status"] = json!("open");
                point["facts"] = json!([{"text": goal, "source": "src/a.rs:1"}]);
                let opened = write(root, "x", "point", point);
                let closing = json!({"block": item["block"], "gap": item["gap"], "from": "gap",
                    "status": "not_applicable", "closes": id_of(&opened), "reason": "Já respondido.", "origin": said});
                assert_eq!(write(root, "x", "point", closing)["ok"], json!(true));
            }
            before(said);
        });
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let first = |kind: &str| log.visible().into_iter().find(|e| e.event_type == kind).map(|e| e.id).unwrap();
        (first("message"), first("criterion"))
    }

    /// Uma tarefa do backlog, gravada pela porta do modelo, num arquivo só.
    fn backlog_task(root: &Path, said: u64, crit: u64, text: &str, file: &str) -> u64 {
        id_of(&write(root, "x", "task", json!({"text": text, "files": [{"path": file}], "depends_on": [],
            "covers": [crit], "origin": said})))
    }

    /// A spec reaberta, de volta ao levantamento, e o plano rodado de novo
    /// nela, com as anotações que ele gravou.
    fn replanned(root: &Path) -> (Value, Vec<Value>) {
        let reopened = crate::commands::flow::reopen::reopen_for(
            &crate::commands::flow::reopen::ReopenOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                reason: "Mais trabalho na mesma obra.".into(),
                fix: false,
            },
            None,
        );
        assert_eq!(reopened["ok"], json!(true), "{reopened}");
        let report = crate::commands::flow::plan::plan_for(
            &crate::commands::flow::plan::PlanOpts { root: root.to_path_buf(), spec: Some("x".into()) },
            None,
        );
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let notes = log.visible().into_iter().filter(|e| e.event_type == "note").map(|e| Value::Object(e.fields.clone())).collect();
        (report, notes)
    }

    /// Os motivos de desenho de onda que o plano disse: na resposta, entre as
    /// travas e os avisos, e nas anotações que ele gravou.
    fn wave_drawing_said(report: &Value, notes: &[Value]) -> Vec<String> {
        let answered = ["blocking", "warnings"]
            .iter()
            .flat_map(|field| report[*field].as_array().cloned().unwrap_or_default())
            .filter_map(|f| f["reason"].as_str().map(str::to_string));
        let noted = notes
            .iter()
            .flat_map(|n| n["keys"].as_array().cloned().unwrap_or_default())
            .filter_map(|k| k.as_str().map(str::to_string));
        answered.chain(noted).filter(|r| WAVE_DRAWING_REASONS.contains(&r.as_str())).collect()
    }

    /// Numa spec reaberta, a primeira rodada leva a tarefa de `src/a.rs` na
    /// onda 1, que entrega; a segunda leva numa onda só uma tarefa nova de
    /// `src/a.rs` e outra de `src/b.rs`. As duas ondas de lote nascem sem
    /// dependência entre elas e tocam o mesmo arquivo, e a segunda tem duas
    /// tarefas que não dividem arquivo: o plano rodado depois não fala de
    /// ondas dividindo arquivo, nem de onda ou spec que podia sair dividida.
    #[test]
    fn o_plano_nao_fala_de_onda_dividindo_arquivo() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let record = write(root, "x", "delivered", json!({"wave": 1, "text": "A onda 1 saiu.", "files": ["src/a.rs"]}));
        assert_eq!(record["ok"], json!(true), "{record}");

        let again = backlog_task(root, said, crit, "Mexer de novo no código de um.", "src/a.rs");
        let other = backlog_task(root, said, crit, "Mexer no código de dois.", "src/b.rs");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(dispatch_backlog(root, "x", &log, &log), Ok(vec![2]), "as duas tarefas novas viram um lote só");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let waves: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "wave").collect();
        assert_eq!(waves.len(), 2, "{waves:?}");
        assert!(waves.iter().all(|w| w.ints("depends_on").is_empty() && w.str_field("author") == Some("binary")));
        let order: Vec<u64> = waves[1].ints("order");
        assert_eq!(order, vec![again, other], "a onda 2 leva as duas tarefas");

        let (report, notes) = replanned(root);
        assert_eq!(wave_drawing_said(&report, &notes), Vec::<String>::new(), "{report}");
    }

    /// Todas as linhas do arquivo de eventos da spec `x` do tipo `kind`,
    /// também as versões já substituídas, na ordem em que foram gravadas.
    fn every_line_of(root: &Path, kind: &str) -> Vec<Value> {
        let text = std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap();
        text.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|event| event["type"] == json!(kind))
            .collect()
    }

    /// Duas rodadas ao mesmo tempo, com uma tarefa pronta no backlog, leem a
    /// spec antes de qualquer uma pegar a trava. A que pega primeiro forma a
    /// onda de lote e a solta; a outra relê a spec já com a trava presa, acha
    /// a tarefa na onda formada e não forma nem solta nada. Sai uma onda só,
    /// com um envio só, e a tarefa ganha uma versão só com o número da onda.
    #[test]
    fn two_rounds_at_once_form_and_send_a_single_batch_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");

        let outs = two_rounds_at_once(root, None);
        for out in &outs {
            assert_eq!(out["ok"], json!(true), "{outs:?}");
        }
        let sent: Vec<u64> = outs.iter().flat_map(|out| waves_in(out, "dispatch")).collect();
        assert_eq!(sent, vec![1], "só uma das rodadas solta a onda: {outs:?}");
        let waves: Vec<Value> = every_line_of(root, "wave").iter().map(|w| w["n"].clone()).collect();
        assert_eq!(waves, vec![json!(1)], "a onda de lote é gravada uma vez: {outs:?}");
        let sends: Vec<Value> = every_line_of(root, "send").iter().map(|s| s["wave"].clone()).collect();
        assert_eq!(sends, vec![json!(1)], "um envio só: {outs:?}");
        let numbered = every_line_of(root, "task").iter().filter(|t| t["wave"] == json!(1)).count();
        assert_eq!(numbered, 1, "a tarefa ganha o número da onda uma vez: {outs:?}");
    }

    /// A rodada forma o lote com a tarefa pronta nas duas leituras — a da
    /// entrada e a feita com a trava presa — e numera a onda nova pela
    /// segunda. A tarefa um estava pronta na entrada desta rodada, mas outra
    /// rodada já a levou na onda 1 antes de esta pegar a trava; a tarefa dois,
    /// pronta nas duas, sai sozinha na onda 2. A tarefa três, gravada depois
    /// da entrada, só está pronta na leitura com a trava e espera a rodada
    /// seguinte.
    #[test]
    fn the_batch_takes_only_the_task_ready_in_both_readings_and_numbers_after_the_lock() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let path = store::spec_file(root, "x").unwrap();
        let one = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let first_entry = store::read(&path).unwrap().unwrap();
        let two = backlog_task(root, said, crit, "Mexer no código de dois.", "src/b.rs");
        let second_entry = store::read(&path).unwrap().unwrap();

        // A outra rodada, que entrou antes da tarefa dois, forma a onda 1 com a
        // tarefa um e solta a trava.
        assert_eq!(dispatch_backlog(root, "x", &first_entry, &first_entry), Ok(vec![1]));
        let three = backlog_task(root, said, crit, "Mexer de novo no código de dois.", "src/b.rs");

        let locked = store::read(&path).unwrap().unwrap();
        assert_eq!(dispatch_backlog(root, "x", &second_entry, &locked), Ok(vec![2]), "a onda nova é a 2");

        let log = store::read(&path).unwrap().unwrap();
        let order = |n: u64| {
            let wave = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(n));
            wave.map(|w| w.ints("order")).unwrap_or_default()
        };
        assert_eq!(order(1), vec![one], "a onda 1 segue só com a tarefa um");
        assert_eq!(order(2), vec![two], "a onda 2 leva só a tarefa pronta nas duas leituras");
        let waves: Vec<Value> = every_line_of(root, "wave").iter().map(|w| w["n"].clone()).collect();
        assert_eq!(waves, vec![json!(1), json!(2)], "nenhum número de onda se repete");
        let task_three = log.current(three).expect("a tarefa três segue viva");
        assert_eq!(task_three.wave(), None, "a tarefa três fica no backlog para a rodada seguinte");
    }

    /// A tarefa pronta quando a rodada entrou ganha versão nova — outro texto,
    /// outro número — antes de a rodada pegar a trava. O lote sai com a
    /// versão em vigor: a onda nova leva o número novo, e a tarefa, com o
    /// texto revisto, ganha o número da onda. A prova atravessa a rodada de
    /// verdade, com a revisão entre a leitura de entrada e o resto dela.
    #[test]
    fn a_task_revised_between_the_two_readings_goes_out_in_the_batch_with_its_current_version() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let first = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");

        let entry = round_entry(root, None);
        let revised = id_of(&write(root, "x", "task", json!({"replaces": first, "text": "Mexer de outro jeito no código de um.",
            "files": [{"path": "src/a.rs"}], "depends_on": [], "covers": [crit], "origin": said})));
        let out = round_from(root, None, entry);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "a tarefa revista sai no lote: {out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let wave = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(1));
        assert_eq!(wave.map(|w| w.ints("order")), Some(vec![revised]), "a onda leva o número da versão em vigor");
        let task = log.current(revised).expect("a tarefa segue viva");
        assert_eq!(task.wave(), Some(1), "a tarefa ganha o número da onda");
        assert_eq!(task.str_field("text"), Some("Mexer de outro jeito no código de um."), "com o texto revisto");
    }

    /// A tarefa da onda de lote em andamento ganha versão nova, pela porta do
    /// modelo, com um texto que não casa com o da onda — o texto da onda de
    /// lote é a junção dos textos da versão velha. O plano rodado depois não
    /// trava, e nenhum aviso fala de tarefa na onda errada ou sem onda.
    #[test]
    fn o_plano_nao_confere_o_texto_da_tarefa_contra_a_onda() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(waves_in_progress(&log).contains_key(&1), "a onda 1 está em andamento");
        let current = log.current(task).expect("a tarefa tem versão vigente");
        assert_eq!(current.wave(), Some(1), "o lote deu a ela o número da onda");
        let revised = crate::commands::spec_events::write::write_at(&crate::commands::spec_events::write::WriteOpts {
            root: root.to_path_buf(),
            spec: Some("x".into()),
            event_type: "task".into(),
            json: json!({"replaces": current.id, "wave": 1, "title": "Somar", "text": "Somar dois números inteiros.",
                "agent": "- somar", "files": [{"path": "src/a.rs"}], "depends_on": [], "covers": [crit], "origin": said})
            .to_string(),
        });
        assert_eq!(revised["ok"], json!(true), "{revised}");

        let (report, notes) = replanned(root);
        assert_eq!(report["blocking"], Value::Null, "o plano não trava: {report}");
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(wave_drawing_said(&report, &notes), Vec::<String>::new(), "{report}");
    }

    /// Duas tarefas soltas do backlog formariam um lote só, e a versão nova da
    /// segunda, com o número da onda, seria recusada: num caso ela traz um
    /// campo que esta versão não conhece, gravado por um Mustard mais novo; no
    /// outro, não declara as dependências, como uma tarefa de antes da regra.
    /// A rodada responde a recusa da gravação e não grava nada: nem a onda do
    /// lote, nem a versão da primeira tarefa. A spec fica igual byte a byte.
    /// A prova atravessa `round`, o comando de verdade.
    #[test]
    fn a_rodada_recusada_nao_deixa_onda_gravada() {
        let cases: [(&str, &str, &str); 2] = [
            ("campo novo", ",\"text\":", "unknown-field"),
            ("sem dependências", ",\"depends_on\":[]", "task-declaration-missing"),
        ];
        for (case, cut, reason) in cases {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let (said, crit) = backlog_project(root);
            backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
            let bad = backlog_task(root, said, crit, "Mexer no código de dois.", "src/b.rs");
            // A spec já está em execução: a rodada não tem fase a gravar antes
            // de formar o lote.
            assert!(crate::commands::spec_events::write::record_phase(root, "x", "running", None), "{case}");

            let path = store::spec_file(root, "x").unwrap();
            let mark = format!("\"id\":{bad},");
            let original = std::fs::read_to_string(&path).unwrap();
            let edited: String = original
                .lines()
                .map(|line| match (line.contains(&mark), reason) {
                    (true, "unknown-field") => line.replacen(cut, ",\"futuro\":1,\"text\":", 1),
                    (true, _) => line.replacen(cut, "", 1),
                    (false, _) => line.to_string(),
                })
                .map(|line| line + "\n")
                .collect();
            assert_ne!(edited, original, "{case}: a linha da segunda tarefa mudou");
            std::fs::write(&path, &edited).unwrap();

            let out = round(root, "x", None);

            assert_eq!(out["ok"], json!(false), "{case}: {out}");
            assert_eq!(out["reason"], json!(reason), "{case}: {out}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), edited, "{case}: a spec fica igual byte a byte");
            let log = store::read(&path).unwrap().unwrap();
            assert!(log.visible().iter().all(|e| e.event_type != "wave"), "{case}: nenhuma onda gravada");
        }
    }

    /// Uma tarefa de limpeza do backlog — nasceu de uma sobra que só muda
    /// comentário —, gravada pela porta do modelo, num arquivo só.
    fn cleanup_task(root: &Path, said: u64, crit: u64, text: &str, file: &str) -> u64 {
        id_of(&write(root, "x", "task", json!({"text": text, "files": [{"path": file}], "depends_on": [],
            "covers": [crit], "origin": said, "cleanup": true})))
    }

    /// A spec `x` como está no arquivo agora.
    fn spec_now(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// A ordem gravada na onda `n` da spec `x`.
    fn wave_order(root: &Path, n: u64) -> Vec<u64> {
        let log = spec_now(root);
        let wave = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(n));
        wave.map(|w| w.ints("order")).unwrap_or_default()
    }

    /// Com uma tarefa comum e uma limpeza prontas no backlog, a rodada solta
    /// só a comum: a limpeza segue no backlog, sem onda.
    #[test]
    fn a_limpeza_fica_no_backlog_enquanto_ha_outra_tarefa_por_fazer() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let normal = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let tidy = cleanup_task(root, said, crit, "Acertar o comentário de dois.", "src/b.rs");

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert_eq!(wave_order(root, 1), vec![normal], "a onda 1 leva só a tarefa comum");
        assert_eq!(spec_now(root).current(tidy).and_then(SpecEvent::wave), None, "a limpeza segue sem onda");
    }

    /// A limpeza espera a onda da tarefa comum terminar. Com a onda 1 no ar,
    /// a rodada não forma onda; a que assume a entrega da 1 não solta nada e
    /// manda rodar de novo; a seguinte solta a limpeza sozinha na onda 2, e
    /// entregue a 2 a rodada manda fechar.
    #[test]
    fn entregue_a_tarefa_normal_a_limpeza_sai_na_rodada_seguinte() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let normal = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let tidy = cleanup_task(root, said, crit, "Acertar o comentário de dois.", "src/b.rs");
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        assert_eq!(wave_order(root, 1), vec![normal], "{first}");

        let idle = round(root, "x", None);
        assert_eq!(idle["ok"], json!(true), "{idle}");
        let formed: Vec<Value> = every_line_of(root, "wave").iter().map(|w| w["n"].clone()).collect();
        assert_eq!(formed, vec![json!(1)], "com a onda 1 no ar, nenhuma onda nova: {idle}");

        let assumed = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(assumed["ok"], json!(true), "{assumed}");
        assert_eq!(waves_in(&assumed, "dispatch"), Vec::<u64>::new(), "{assumed}");
        assert_eq!(assumed["command"], json!("mustard-rt run round --spec x"), "{assumed}");

        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![2], "a limpeza sai na onda 2: {again}");
        assert_eq!(wave_order(root, 2), vec![tidy], "a onda 2 leva só a limpeza");

        let done = round(root, "x", Some(&delivered(root, 2, "Saiu.", &["src/b.rs"])));
        assert_eq!(done["command"], json!("mustard-rt run close --spec x"), "{done}");
    }

    /// Uma limpeza a mais que o teto de tarefas do lote, cada uma num
    /// arquivo próprio: todas saem juntas numa onda só.
    #[test]
    fn as_limpezas_saem_juntas_numa_onda_mesmo_passando_do_teto_do_lote() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let tidy: Vec<u64> = (1..=crate::shared::dag::BATCH_CAP.tasks + 1)
            .map(|n| cleanup_task(root, said, crit, &format!("Acertar o comentário {n}."), &format!("src/c{n}.rs")))
            .collect();
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log), Ok(vec![1]), "uma onda só");
        assert_eq!(wave_order(root, 1), tidy, "a onda 1 leva todas as limpezas");
    }

    /// Com a onda da tarefa comum entregue e a limpeza ainda no backlog, o
    /// fechamento recusa pelo backlog que não esvaziou.
    #[test]
    fn o_fechamento_recusa_com_limpeza_por_fazer() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        cleanup_task(root, said, crit, "Acertar o comentário de dois.", "src/b.rs");
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let assumed = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(assumed["ok"], json!(true), "{assumed}");

        assert_eq!(
            crate::commands::flow::close::finished_refusal(&spec_now(root)),
            Some(("backlog-not-empty".into(), None)),
            "{assumed}"
        );
    }

    /// As versões de tarefa com o código `code` que a leitura mostra.
    fn shown_task_versions(log: &SpecLog, code: &str) -> Vec<u64> {
        let codes = log.codes();
        log.visible()
            .into_iter()
            .filter(|e| e.event_type == "task" && codes.get(&e.id).map(String::as_str) == Some(code))
            .map(|e| e.id)
            .collect()
    }

    /// A tarefa `task` solta pela rodada numa onda de lote, e a versão dela
    /// que a rodada gravou com o número da onda.
    fn batched(root: &Path, task: u64) -> u64 {
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log), Ok(vec![1]), "a tarefa sai na onda 1");
        let log = spec_now(root);
        let version = log.current(task).expect("a tarefa segue viva");
        assert_eq!(version.wave(), Some(1), "a rodada gravou a versão com a onda");
        version.id
    }

    /// O usuário remove, pelo número, a versão que a rodada gravou com a
    /// onda. A tarefa sai inteira: a versão sem onda não volta ao backlog, a
    /// onda que ficou vazia sai do plano, e a rodada seguinte não forma lote.
    #[test]
    fn removing_the_round_version_of_a_task_takes_out_the_whole_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        let code = spec_now(root).codes().get(&task).cloned().expect("a tarefa tem código");

        let removed = write(root, "x", "remove", json!({"targets": [with_wave], "reason": "Não é mais preciso."}));
        assert_eq!(removed["ok"], json!(true), "{removed}");

        let log = spec_now(root);
        assert_eq!(shown_task_versions(&log, &code), Vec::<u64>::new(), "nenhuma versão da tarefa fica na leitura");
        assert!(!log.planned_waves().contains(&1), "a onda sem tarefa sai do plano");
        assert_eq!(dispatch_backlog(root, "x", &log, &log), Ok(vec![]), "nada volta ao backlog");
    }

    /// A tarefa tem três versões: a sem onda, a da rodada e a do agente, com
    /// texto novo. Remover pelo número a do meio tira as três.
    #[test]
    fn removing_the_middle_version_of_a_task_takes_out_every_version() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        let code = spec_now(root).codes().get(&task).cloned().expect("a tarefa tem código");
        let by_agent = id_of(&write(root, "x", "task", json!({"replaces": with_wave, "wave": 1,
            "text": "Mexer de outro jeito no código de um.", "files": [{"path": "src/a.rs"}],
            "depends_on": [], "covers": [crit], "origin": said})));
        assert_eq!(shown_task_versions(&spec_now(root), &code), vec![by_agent], "a versão do agente é a vigente");

        let removed = write(root, "x", "remove", json!({"targets": [with_wave], "reason": "Não é mais preciso."}));
        assert_eq!(removed["ok"], json!(true), "{removed}");

        let log = spec_now(root);
        assert_eq!(shown_task_versions(&log, &code), Vec::<u64>::new(), "as três versões saem");
        assert_eq!(dispatch_backlog(root, "x", &log, &log), Ok(vec![]), "nada volta ao backlog");
    }

    /// A rodada monta a versão nova de uma tarefa com a onda 2, e antes de
    /// ela gravar, o usuário remove a tarefa pelo código. A gravação da
    /// rodada é recusada, e a tarefa segue fora.
    #[test]
    fn the_round_version_written_after_the_removal_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        let log = spec_now(root);
        let code = log.codes().get(&task).cloned().expect("a tarefa tem código");
        let draft = task_revision(&log, with_wave, Map::from_iter([("wave".to_string(), json!(2))])).expect("o rascunho");

        let removed = write(root, "x", "remove", json!({"targets": [code], "reason": "Não é mais preciso."}));
        assert_eq!(removed["ok"], json!(true), "{removed}");

        let Err(refusal) = record(root, "x", "task", draft, PhaseWriter::Binary) else {
            panic!("a versão nova sobre a tarefa removida foi gravada")
        };
        assert_eq!(refusal.reason(), "replaces-removed", "{refusal:?}");
        let log = spec_now(root);
        assert_eq!(shown_task_versions(&log, &code), Vec::<u64>::new(), "a tarefa segue fora");
        assert_eq!(dispatch_backlog(root, "x", &log, &log), Ok(vec![]), "nada volta ao backlog");
    }

    /// A rodada monta a versão nova de uma tarefa, e antes de ela gravar,
    /// outra gravação revê a tarefa com texto novo. A versão da rodada é
    /// recusada como substituída, e o texto novo segue na leitura.
    #[test]
    fn a_task_revised_after_the_round_reading_keeps_its_new_text() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let draft = task_revision(&spec_now(root), task, Map::from_iter([("wave".to_string(), json!(1))])).expect("o rascunho");
        let revised = id_of(&write(root, "x", "task", json!({"replaces": task, "text": "Mexer de outro jeito no código de um.",
            "files": [{"path": "src/a.rs"}], "depends_on": [], "covers": [crit], "origin": said})));

        let Err(refusal) = record(root, "x", "task", draft, PhaseWriter::Binary) else {
            panic!("a versão da rodada passou por cima do texto novo")
        };
        assert_eq!(refusal.reason(), "replaces-superseded", "{refusal:?}");
        let log = spec_now(root);
        assert_eq!(log.current(task).map(|e| e.id), Some(revised), "a versão com texto novo segue vigente");
    }

    /// Com a tarefa removida pela versão da rodada e outra tarefa revista uma
    /// vez, cada versão que a rodada seguinte grava aponta a versão mais nova
    /// do mesmo código.
    #[test]
    fn the_round_version_replaces_the_newest_version_of_the_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        write(root, "x", "remove", json!({"targets": [with_wave], "reason": "Não é mais preciso."}));
        let other = backlog_task(root, said, crit, "Mexer no código de dois.", "src/b.rs");
        id_of(&write(root, "x", "task", json!({"replaces": other, "text": "Mexer de outro jeito no código de dois.",
            "files": [{"path": "src/b.rs"}], "depends_on": [], "covers": [crit], "origin": said})));

        let before = spec_now(root);
        let codes = before.codes();
        let newest = |code: &str| before.events.iter().filter(|e| codes.get(&e.id).map(String::as_str) == Some(code)).map(|e| e.id).max();
        assert_eq!(dispatch_backlog(root, "x", &before, &before), Ok(vec![2]), "a outra tarefa sai na onda 2");

        let after = spec_now(root);
        let written: Vec<&SpecEvent> =
            after.events.iter().filter(|e| e.id > before.max_id() && e.event_type == "task").collect();
        let after_codes = after.codes();
        for version in &written {
            let code = after_codes.get(&version.id).expect("a versão tem código");
            assert_eq!(version.int("replaces"), newest(code), "a versão da rodada aponta a mais nova de {code}");
        }
        assert_eq!(written.len(), 1, "só a outra tarefa ganha versão nova");
    }

    /// Numa spec antiga, a remoção de só a versão com onda devolveu a versão
    /// sem onda ao backlog. A rodada não a solta de novo: a versão nova
    /// aponta a versão removida, e a gravação a recusa sem gravar nada.
    #[test]
    fn an_old_removal_of_the_round_version_does_not_send_the_task_again() {
        use std::io::Write as _;

        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        let path = store::spec_file(root, "x").unwrap();
        let by = spec_now(root).max_id() + 1;
        let old_removal = json!({"v": 1, "id": by, "at": "2026-09-20T10:00:00-03:00", "type": "remove",
            "targets": [with_wave], "reason": "Não é mais preciso.", "gives_back": true, "author": "assistant"});
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{old_removal}").unwrap();

        let log = spec_now(root);
        assert_eq!(log.current(task).map(|e| e.id), Some(task), "a leitura antiga devolve a versão sem onda");
        assert_eq!(
            dispatch_backlog(root, "x", &log, &log),
            Err(mustard_core::domain::spec_events::Refusal::ReplacesRemoved { id: with_wave, by }),
            "a rodada não solta a tarefa removida"
        );
        assert_eq!(spec_now(root).max_id(), by, "nada foi gravado");
    }

    /// A tarefa de uma onda de lote que ainda não saiu ganha, por uma versão
    /// nova pela porta do modelo, a dependência de uma tarefa do backlog. Com
    /// a onda, a gravação é recusada com o conserto, e nada é gravado; sem a
    /// onda, passa, e a rodada seguinte solta a dependência antes dela, no
    /// mesmo lote.
    #[test]
    fn a_wave_task_that_gains_a_backlog_dependency_goes_out_after_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        let dependency = backlog_task(root, said, crit, "Preparar o código de um.", "src/a.rs");
        let version = |wave: Option<u64>| {
            let mut body = json!({"replaces": with_wave, "text": "Mexer no código de um.", "files": [{"path": "src/a.rs"}],
                "depends_on": [dependency], "covers": [crit], "origin": said});
            if let Some(n) = wave {
                body["wave"] = json!(n);
            }
            write(root, "x", "task", body)
        };

        let before = spec_now(root).max_id();
        let refused = version(Some(1));
        assert_eq!(refused["ok"], json!(false), "{refused}");
        assert_eq!(refused["reason"], json!("depends-outside-wave"), "{refused}");
        assert!(refused["hint"].as_str().is_some_and(|hint| hint.contains("sem wave")), "{refused}");
        assert_eq!(spec_now(root).max_id(), before, "nada foi gravado");

        let accepted = id_of(&version(None));
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "{out}");
        assert_eq!(wave_order(root, 2), vec![dependency, accepted], "a dependência sai antes da tarefa");
    }

    /// Três tarefas do backlog no mesmo arquivo, cada uma esperando a
    /// anterior: o lote leva as três, na ordem da cadeia, e a versão com a
    /// onda de cada uma passa na conferência da gravação.
    #[test]
    fn a_backlog_batch_with_a_chain_inside_it_passes_the_check() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let first = backlog_task(root, said, crit, "Preparar o código de um.", "src/a.rs");
        let after = |on: u64, text: &str| {
            id_of(&write(root, "x", "task", json!({"text": text, "files": [{"path": "src/a.rs"}], "depends_on": [on],
                "covers": [crit], "origin": said})))
        };
        let second = after(first, "Mexer no código de um.");
        let third = after(second, "Testar o código de um.");

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log), Ok(vec![1]), "a cadeia vira um lote só");
        assert_eq!(wave_order(root, 1), vec![first, second, third], "na ordem da cadeia");
        let log = spec_now(root);
        for task in [first, second, third] {
            assert_eq!(log.current(task).and_then(SpecEvent::wave), Some(1), "a tarefa {task} ganhou a onda");
        }
    }

    /// O backlog da onda que volta sem fazer uma das tarefas, depois do
    /// "Aceitar" do usuário na mudança de plano dela.
    pub(crate) struct UndoneReturn {
        /// O código de cada tarefa: A, B, C e D.
        pub(crate) a: String,
        pub(crate) b: String,
        pub(crate) c: String,
        pub(crate) d: String,
        /// O código da decisão que só B cobre.
        pub(crate) b_decision: String,
        /// A mudança de plano que a onda propôs.
        pub(crate) change: String,
        /// A resposta da rodada que parou pela mudança, antes do clique.
        pub(crate) stopped: Value,
        /// A resposta da rodada que assumiu a volta, depois do clique.
        pub(crate) accepted: Value,
    }

    /// A versão vigente da tarefa de código `code` na spec `x`.
    pub(crate) fn task_now(root: &Path, code: &str) -> SpecEvent {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        log.visible()
            .into_iter()
            .find(|e| e.event_type == "task" && codes.get(&e.id).map(String::as_str) == Some(code))
            .cloned()
            .unwrap_or_else(|| panic!("sem a tarefa {code}"))
    }

    /// Um backlog com quatro tarefas, cada uma num arquivo: A cobre uma
    /// decisão e B cobre outra; C depende de B, e D depende de A. Nem C nem D
    /// divide arquivo com A ou com B, e por isso nenhuma entra no lote de A e
    /// B pela espera. A primeira rodada leva A e B numa onda de lote só; a
    /// onda volta com A feita e B por fazer, com a mudança de plano, a rodada
    /// para, o usuário clica em "Aceitar", e a rodada seguinte assume a
    /// volta.
    pub(crate) fn return_with_an_undone_task(root: &Path) -> UndoneReturn {
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["c.rs", "d.rs"] {
            std::fs::write(root.join("src").join(name), "fn um() {}\n").unwrap();
        }
        let decisions = std::cell::Cell::new((0, 0));
        let (said, crit) = backlog_project_with(root, |said| {
            let decision = |text: &str| {
                id_of(&write(root, "x", "decision", json!({"text": text, "why": "w", "keys": ["k"], "origin": said})))
            };
            decisions.set((decision("A soma arredonda para baixo."), decision("A lista vazia soma zero.")));
        });
        let (for_a, for_b) = decisions.get();
        let task = |text: &str, file: &str, covers: Vec<u64>, depends: Vec<u64>| {
            id_of(&write(root, "x", "task", json!({"text": text, "files": [{"path": file}], "depends_on": depends,
                "covers": covers, "origin": said})))
        };
        let a = task("Arredondar a soma.", "src/a.rs", vec![crit, for_a], vec![]);
        let b = task("Somar a lista vazia.", "src/b.rs", vec![crit, for_b], vec![]);
        let c = task("Mostrar a soma da lista vazia.", "src/c.rs", vec![crit], vec![b]);
        let d = task("Mostrar a soma arredondada.", "src/d.rs", vec![crit], vec![a]);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let code = |id: u64| codes[&id].clone();
        let (a, b, c, d, b_decision) = (code(a), code(b), code(c), code(d), code(for_b));

        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let batch: Vec<String> = [&a, &b, &c, &d]
            .into_iter()
            .filter(|task| task_now(root, task).wave() == Some(1))
            .cloned()
            .collect();
        assert_eq!(batch, vec![a.clone(), b.clone()], "o lote leva A e B: {first}");

        let session = "s-tarefa-nao-feita";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");
        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma arredonda.\n").unwrap();
        let change = "B precisa de uma decisão sobre a lista vazia antes.";
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let agreed: Vec<Value> = super::super::report::request_agreed(&log, 1, &Languages::of_project(root))
            .iter()
            .map(|item| {
                let code = log.codes()[&item.id].clone();
                if code == b_decision {
                    json!({"item": code, "met": false, "text": "B não foi feita."})
                } else {
                    json!({"item": code, "met": true})
                }
            })
            .collect();
        assert_eq!(agreed.len(), 2, "o pedido leva as duas decisões: {agreed:?}");
        let back = json!({"wave": 1, "text": "A saiu; B ficou por fazer.", "files": ["src/a.rs"],
            "commit": "a soma arredonda", "replan": change, "undone": [b], "agreed": agreed});
        let wrote = returned(root, back);
        assert_eq!(wrote["ok"], json!(true), "{wrote}");

        let stopped = round(root, "x", None);
        assert_eq!(stopped["reason"], json!("wave-plan-does-not-work"), "{stopped}");
        let question = stopped["question"].as_str().unwrap_or_default().to_string();
        let header = stopped["header"].as_str().unwrap_or_default().to_string();
        click(root, session, &question, &header, "Aceitar");
        let accepted = round(root, "x", None);
        assert_eq!(accepted["ok"], json!(true), "{accepted}");
        UndoneReturn { a, b, c, d, b_decision, change: change.to_string(), stopped, accepted }
    }

    /// A tarefa que a onda não fez volta ao backlog sem a onda que a levou,
    /// e só sai na rodada seguinte à que a soltou. A tarefa que depende dela
    /// segue esperando, e a que dependia só da tarefa feita sai no mesmo lote
    /// da devolvida. A onda entregue não sai de novo.
    #[test]
    fn a_task_the_wave_did_not_do_goes_back_to_the_queue_and_holds_its_dependents() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        let UndoneReturn { a, b, c, d, accepted, .. } = return_with_an_undone_task(root);

        assert_eq!(task_now(root, &a).wave(), Some(1), "A segue na onda que a fez");
        let returned_b = task_now(root, &b);
        assert_eq!(returned_b.wave(), None, "B voltou sem onda: {:?}", returned_b.fields);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(backlog_left(&log).contains(&returned_b.id), "B está no backlog");
        assert_eq!(waves_in(&accepted, "dispatch"), Vec::<u64>::new(), "B só sai na rodada seguinte: {accepted}");

        let next = round(root, "x", None);
        assert_eq!(waves_in(&next, "dispatch"), vec![2], "só o lote novo sai, nunca a onda entregue: {next}");
        let in_two: Vec<String> = [&a, &b, &c, &d]
            .into_iter()
            .filter(|task| task_now(root, task).wave() == Some(2))
            .cloned()
            .collect();
        assert_eq!(in_two, vec![b.clone(), d.clone()], "o lote leva B e D: {next}");
        assert_eq!(task_now(root, &c).wave(), None, "C espera B ser entregue");

        let quiet = round(root, "x", None);
        assert!(!waves_in(&quiet, "dispatch").contains(&1), "a onda entregue não sai de novo: {quiet}");
    }
}
