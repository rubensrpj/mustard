//! A conversão da spec antiga para o backlog: a onda desenhada à mão que nunca
//! saiu deixa de valer, e as tarefas dela voltam para o backlog, de onde o
//! programa monta os lotes.
//!
//! **Quem é convertida.** Toda onda visível com autor diferente do programa,
//! sem envio, sem entrega e sem veredito aprovado, cujo número não seja
//! também o de uma onda que o programa montou. A onda entregue ou aprovada
//! fica como história; a que já saiu termina como saiu.
//!
//! **O que se grava.** Para cada onda convertida, uma remoção com autor do
//! programa e o motivo em palavras, que a página mostra na lista do que saiu.
//! A remoção aponta a onda pelo código e tira todas as versões dela.
//! Cada tarefa dela ganha uma versão nova sem o número de onda e sem a nota:
//! a dependência entre ondas vira dependência das tarefas da onda de que ela
//! dependia; a tarefa sem arquivo leva os da onda, e sem nenhum leva o
//! curinga; a sem dependência leva a lista vazia. Cada critério que uma onda
//! convertida listava fica coberto por uma tarefa só — a última da onda que o
//! listava por último na ordem das dependências —, e essa tarefa passa a
//! depender das outras tarefas de todas as ondas que o listavam: o critério
//! só roda quando tudo o que ele cobre entregou.
//!
//! O item combinado que diz a onda convertida em `waves` ganha uma versão
//! nova sem ela, com o dono pelos arquivos: `applies_to` leva os arquivos das
//! tarefas das ondas convertidas que ele dizia, e o item vai no pedido do lote
//! que tocar um deles. A onda que ele diz e que fica como história continua
//! em `waves`.
//!
//! **Sem meio caminho.** Tudo passa antes pela conferência inteira da
//! gravação, que também recusa um círculo no grafo das tarefas, e só então
//! é gravado, com a trava do passo do git presa só durante a conversão. A
//! remoção vai primeiro: a onda que ela tirou continua no arquivo, e é por ela
//! que a conversão que parou no meio se completa na chamada seguinte, só com
//! o que falta. Numa spec já convertida, nada é gravado.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::spec_events::{Block, EventRef, Hidden, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::report::backlog_return;
use crate::commands::spec_events::write::{record, RecordCheck};

/// O arquivo que a tarefa sem arquivo nenhum leva, nem dela nem da onda.
const ANY_FILE: &str = "**";

/// Converte a spec `spec`, vista de `start`, no checkout `root`, e devolve
/// os números do que gravou, na ordem; vazio quando não havia o que
/// converter.
///
/// # Errors
///
/// A recusa da trava, da leitura, da conferência ou da primeira gravação
/// que falhar; a conferência recusa antes de qualquer gravação.
pub(crate) fn convert_hand_waves(start: &Path, root: &Path, spec: &str, lang: Locale) -> Result<Vec<u64>, Refusal> {
    let _held = crate::commands::git_settle::git_step_lock(root).map_err(|detail| Refusal::Io { detail })?;
    let path = store::spec_file(root, spec)?;
    let Some(log) = store::read(&path)? else { return Ok(Vec::new()) };
    let writes = planned_writes(&log, lang);
    if writes.is_empty() {
        return Ok(Vec::new());
    }
    let mut check = RecordCheck::open(start, spec, PhaseWriter::Binary)?;
    for (event_type, draft) in &writes {
        check.record(event_type, draft.clone())?;
    }
    let mut written = Vec::new();
    for (event_type, draft) in writes {
        written.push(record(start, spec, &event_type, draft, PhaseWriter::Binary)?.written.id);
    }
    Ok(written)
}

/// Uma onda convertida: o evento que a descreve e se ele ainda está na
/// leitura, esperando a remoção.
struct HandWave<'a> {
    event: &'a SpecEvent,
    visible: bool,
}

/// As ondas convertidas, pelo número: as que esta chamada remove e as que
/// uma conversão anterior já removeu.
fn hand_waves(log: &SpecLog) -> BTreeMap<u64, HandWave<'_>> {
    let visible: BTreeSet<u64> = log.visible().iter().map(|e| e.id).collect();
    let hidden = log.hidden();
    let by_binary = |event: &SpecEvent| event.str_field("author") == Some("binary");
    let binary_numbers: BTreeSet<u64> = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "wave" && by_binary(e))
        .filter_map(SpecEvent::wave)
        .collect();
    let went_out: BTreeSet<u64> = log
        .visible()
        .into_iter()
        .filter(|e| match e.event_type.as_str() {
            "send" | "delivered" => true,
            "verdict" => e.str_field("result") == Some("approved"),
            _ => false,
        })
        .filter_map(SpecEvent::wave)
        .collect();
    let mut out: BTreeMap<u64, HandWave<'_>> = BTreeMap::new();
    for event in log.events.iter().filter(|e| e.event_type == "wave" && !by_binary(e)) {
        let Some(n) = event.wave() else { continue };
        if binary_numbers.contains(&n) || went_out.contains(&n) {
            continue;
        }
        let removed_by_binary = matches!(hidden.get(&event.id), Some(Hidden::Removed { by })
            if log.get(*by).is_some_and(by_binary));
        if visible.contains(&event.id) {
            out.insert(n, HandWave { event, visible: true });
        } else if removed_by_binary && out.get(&n).is_none_or(|w| !w.visible) {
            out.insert(n, HandWave { event, visible: false });
        }
    }
    out
}

/// A versão mais nova de `task` que traz o número de onda, e esse número:
/// é por ela que a tarefa já convertida segue ligada à onda de onde veio.
fn last_wave_version<'a>(log: &'a SpecLog, task: &'a SpecEvent) -> Option<(u64, u64)> {
    let mut at = task;
    for _ in 0..=log.events.len() {
        if let Some(n) = at.int("wave") {
            return Some((at.id, n));
        }
        at = log.get(*at.replaced().first()?)?;
    }
    None
}

/// Os arquivos que uma tarefa declara, na ordem.
fn declared_files(task: &SpecEvent) -> Vec<String> {
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

/// A tarefa vigente que `value` aponta, pelo número ou pelo código.
fn task_ref(log: &SpecLog, codes: &BTreeMap<u64, String>, value: &Value) -> Option<u64> {
    let raw = match EventRef::from_value(value)? {
        EventRef::Id(id) => id,
        EventRef::Code(code) => log
            .events
            .iter()
            .filter(|e| e.event_type == "task" && codes.get(&e.id) == Some(&code))
            .map(|e| e.id)
            .next_back()?,
    };
    log.current(raw).filter(|e| e.event_type == "task").map(|e| e.id)
}

/// O nível de cada onda convertida entre as convertidas: zero sem
/// dependência entre elas; um acima da mais alta de que ela depende. Um
/// círculo entre ondas não trava a conta: quem o recusa é a conferência das
/// tarefas.
fn levels(waves: &BTreeMap<u64, HandWave<'_>>) -> BTreeMap<u64, usize> {
    fn level(n: u64, waves: &BTreeMap<u64, HandWave<'_>>, seen: &mut BTreeSet<u64>, memo: &mut BTreeMap<u64, usize>) -> usize {
        if let Some(found) = memo.get(&n) {
            return *found;
        }
        if !seen.insert(n) {
            return 0;
        }
        let on = waves.get(&n).map(|w| w.event.ints("depends_on")).unwrap_or_default();
        let found = on.into_iter().filter(|d| waves.contains_key(d)).map(|d| level(d, waves, seen, memo) + 1).max().unwrap_or(0);
        memo.insert(n, found);
        found
    }
    let mut memo = BTreeMap::new();
    for n in waves.keys() {
        level(*n, waves, &mut BTreeSet::new(), &mut memo);
    }
    memo
}

/// Tudo o que a conversão grava, na ordem: as remoções e as tarefas.
fn planned_writes(log: &SpecLog, lang: Locale) -> Vec<(String, Map<String, Value>)> {
    let waves = hand_waves(log);
    if waves.is_empty() {
        return Vec::new();
    }
    let codes = log.codes();
    let current = |id: u64| log.current(id).map_or(id, |e| e.id);
    let tasks: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "task").collect();

    // As tarefas de cada onda convertida, na ordem em que nasceram nela.
    let mut members: BTreeMap<u64, Vec<(u64, &SpecEvent)>> = BTreeMap::new();
    for task in &tasks {
        if let Some((key, n)) = last_wave_version(log, task).filter(|(_, n)| waves.contains_key(n)) {
            members.entry(n).or_default().push((key, *task));
        }
    }
    for list in members.values_mut() {
        list.sort_by_key(|(key, _)| *key);
    }
    let tasks_of = |n: u64| -> Vec<u64> {
        match members.get(&n) {
            Some(list) => list.iter().map(|(_, t)| t.id).collect(),
            None if waves.contains_key(&n) => Vec::new(),
            None => tasks.iter().filter(|t| t.wave() == Some(n)).map(|t| t.id).collect(),
        }
    };
    let wave_files: BTreeMap<u64, Vec<String>> = members
        .iter()
        .map(|(n, list)| {
            let mut files: Vec<String> = Vec::new();
            for file in list.iter().flat_map(|(_, t)| declared_files(t)).filter(|f| f != ANY_FILE) {
                if !files.contains(&file) {
                    files.push(file);
                }
            }
            (*n, files)
        })
        .collect();
    let files_of = |task: &SpecEvent, n: u64| -> Vec<String> {
        let own = declared_files(task);
        if !own.is_empty() {
            return own;
        }
        let of_wave = wave_files.get(&n).cloned().unwrap_or_default();
        if of_wave.is_empty() { vec![ANY_FILE.to_string()] } else { of_wave }
    };

    // Cada critério das ondas convertidas, com a tarefa que passa a cobri-lo
    // e as ondas que o listavam.
    let level = levels(&waves);
    let mut listed: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for (n, wave) in &waves {
        for criterion in wave.event.ints("criteria") {
            listed.entry(current(criterion)).or_default().push(*n);
        }
    }
    let mut holder: BTreeMap<u64, (u64, Vec<u64>)> = BTreeMap::new();
    for (criterion, listing) in &listed {
        let mut order = listing.clone();
        order.sort_by_key(|n| (level.get(n).copied().unwrap_or(0), *n));
        let Some(task) = order.iter().rev().find_map(|n| members.get(n).and_then(|l| l.last())).map(|(_, t)| t.id) else {
            continue;
        };
        let others: Vec<u64> = listing.iter().flat_map(|n| tasks_of(*n)).filter(|id| *id != task).collect();
        holder.insert(*criterion, (task, others));
    }

    let mut writes: Vec<(String, Map<String, Value>)> = Vec::new();
    for wave in waves.values().filter(|w| w.visible) {
        // A onda sai pelo código, que a gravação troca por todas as versões
        // dela: pelo número, só a versão à mostra sairia, e a anterior
        // voltaria à leitura.
        let target = codes.get(&wave.event.id).map_or_else(|| json!(wave.event.id), |code| json!(code));
        let draft = json!({
            "targets": [target],
            "reason": translate("wave.hand_drawn_removed", lang),
            "author": "binary",
        });
        let Value::Object(draft) = draft else { unreachable!("json! de um mapa sempre é objeto") };
        writes.push(("remove".to_string(), draft));
    }
    writes.extend(agreed_versions(log, &waves, &wave_files));
    for (n, list) in &members {
        let wave_on = waves.get(n).map(|w| w.event.ints("depends_on")).unwrap_or_default();
        for (_, task) in list.iter().filter(|(_, t)| t.fields.contains_key("wave")) {
            let mut draft = backlog_return(task);
            draft.remove("points");
            let files: Vec<Value> = files_of(task, *n).into_iter().map(|path| json!({ "path": path })).collect();
            draft.insert("files".into(), json!(files));
            let mut on: Vec<u64> = Vec::new();
            let declared = task.fields.get("depends_on").and_then(Value::as_array).cloned().unwrap_or_default();
            let owned: Vec<u64> =
                holder.values().filter(|(holding, _)| *holding == task.id).flat_map(|(_, others)| others.clone()).collect();
            let found = declared.iter().filter_map(|value| task_ref(log, &codes, value));
            for id in found.chain(wave_on.iter().flat_map(|d| tasks_of(*d))).chain(owned) {
                if id != task.id && !on.contains(&id) {
                    on.push(id);
                }
            }
            draft.insert("depends_on".into(), json!(on));
            let mut covers: Vec<u64> =
                task.ints("covers").into_iter().filter(|id| !holder.contains_key(&current(*id))).collect();
            covers.extend(holder.iter().filter(|(_, (holding, _))| *holding == task.id).map(|(criterion, _)| *criterion));
            if !covers.is_empty() || task.fields.contains_key("covers") {
                draft.insert("covers".into(), json!(covers));
            }
            writes.push(("task".to_string(), draft));
        }
    }
    writes
}

/// A versão nova de cada item combinado que diz uma onda convertida em
/// `waves`: sem essas ondas, e com `applies_to` somando aos arquivos que ele
/// já dizia os das tarefas delas. A onda convertida sem arquivo nenhum dá o
/// curinga, como a tarefa sem arquivo. O item que já perdeu as ondas
/// convertidas, numa conversão anterior, não muda.
fn agreed_versions(
    log: &SpecLog,
    waves: &BTreeMap<u64, HandWave<'_>>,
    wave_files: &BTreeMap<u64, Vec<String>>,
) -> Vec<(String, Map<String, Value>)> {
    let mut writes = Vec::new();
    let agreed = log.visible().into_iter().filter(|e| e.block() == Some(Block::Agreed));
    for item in agreed.filter(|e| e.ints("waves").iter().any(|n| waves.contains_key(n))) {
        let said = item.ints("waves");
        let mut files: Vec<String> = item
            .fields
            .get("applies_to")
            .and_then(|at| at.get("files"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        for n in said.iter().filter(|n| waves.contains_key(n)) {
            let of_wave = wave_files.get(n).cloned().unwrap_or_default();
            let of_wave = if of_wave.is_empty() { vec![ANY_FILE.to_string()] } else { of_wave };
            for file in of_wave {
                if !files.contains(&file) {
                    files.push(file);
                }
            }
        }
        let mut draft: Map<String, Value> = item
            .fields
            .iter()
            .filter(|(key, _)| !["v", "id", "code", "at", "type", "search", "author", "waves"].contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let kept: Vec<u64> = said.into_iter().filter(|n| !waves.contains_key(n)).collect();
        if !kept.is_empty() {
            draft.insert("waves".into(), json!(kept));
        }
        let mut at = item.fields.get("applies_to").and_then(Value::as_object).cloned().unwrap_or_default();
        at.insert("files".into(), json!(files));
        draft.insert("applies_to".into(), Value::Object(at));
        draft.insert("replaces".into(), json!(item.id));
        draft.insert("author".into(), json!("binary"));
        writes.push((item.event_type.clone(), draft));
    }
    writes
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::io::spec_events as store;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::*;
    use crate::commands::flow::round::tests::*;
    use crate::shared::spec_state::seed_event;

    /// A spec lida agora.
    fn log_of(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// Uma linha escrita direto no arquivo, como a versão antiga do programa
    /// a deixava: é assim que a tarefa com a nota do Scrum, que a gravação de
    /// hoje recusa, chega à conversão.
    fn old_line(root: &Path, event_type: &str, fields: Value) -> u64 {
        let path = store::spec_file(root, "x").unwrap();
        let id = log_of(root).max_id() + 1;
        let mut line = json!({"v": 1, "id": id, "at": "2026-09-18T10:00:00-03:00", "type": event_type,
            "author": "assistant"});
        for (key, value) in fields.as_object().unwrap() {
            line[key] = value.clone();
        }
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str(&format!("{line}\n"));
        std::fs::write(&path, text).unwrap();
        id
    }

    /// Uma onda desenhada à mão no plano antigo, com autor do assistente.
    fn hand_wave(root: &Path, said: u64, n: u64, criteria: &[u64], depends_on: &[u64]) -> u64 {
        seed_event(root, "x", "wave", json!({"n": n, "text": format!("Onda {n}, à mão."), "criteria": criteria,
            "done_when": "A suíte passa.", "depends_on": depends_on, "origin": said, "author": "assistant"}))
    }

    /// Uma tarefa de onda à mão, com o arquivo e o que ela cobre.
    fn hand_task(root: &Path, said: u64, n: u64, text: &str, file: Option<&str>, covers: &[u64]) -> u64 {
        let mut body = json!({"wave": n, "text": text, "origin": said, "covers": covers});
        if let Some(file) = file {
            body["files"] = json!([{ "path": file }]);
        }
        seed_event(root, "x", "task", body)
    }

    /// Um critério com prova que sempre passa.
    fn criterion(root: &Path, said: u64, then: &str) -> u64 {
        seed_event(root, "x", "criterion", json!({"when": "a onda roda", "then": then, "proof": "git --version",
            "form": "ubiquitous", "origin": said}))
    }

    /// A onda `n` saiu, entregou e foi comitada.
    fn delivered_and_committed(root: &Path, n: u64, file: &str) {
        seed_send(root, n);
        seed_event(root, "x", "delivered", json!({"wave": n, "text": "Pronta.", "files": [file], "author": "wave"}));
        seed_event(root, "x", "commit", json!({"sha": "0123456789abcdef0123456789abcdef01234567",
            "title": format!("a onda {n} saiu"), "waves": [n], "files": [file], "repo": "."}));
    }

    /// Os números das tarefas da spec antiga, pelo texto.
    struct Old {
        wave: BTreeMap<u64, u64>,
        task: BTreeMap<&'static str, u64>,
        shared: u64,
    }

    /// A spec no formato da pi-kpis-plantio: a onda 1 saiu, entregou e foi
    /// comitada; as ondas 2, 3 e 4 foram desenhadas à mão e nunca saíram; a 2
    /// e a 3 dependem da 1, a 4 depende da 2 e da 3; a 3 e a 4 listam o mesmo
    /// critério; uma tarefa da 3 não declara arquivo e a da 2 traz a nota do
    /// Scrum.
    fn old_spec(root: &Path) -> Old {
        for file in ["src/um.rs", "src/dois.rs", "src/tres.rs", "src/quatro_a.rs", "src/quatro_b.rs"] {
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::write(root.join(file), "fn um() {}\n").unwrap();
        }
        let mut old = Old { wave: BTreeMap::new(), task: BTreeMap::new(), shared: 0 };
        approved_with(root, "x", &[], |said| {
            let one = criterion(root, said, "a primeira parte passa");
            let two = criterion(root, said, "a segunda parte passa");
            old.shared = criterion(root, said, "as duas partes de cima conversam");
            old.wave.insert(1, hand_wave(root, said, 1, &[one], &[]));
            old.task.insert("um", hand_task(root, said, 1, "Fazer a parte um.", Some("src/um.rs"), &[one]));
            old.wave.insert(2, hand_wave(root, said, 2, &[two], &[1]));
            let dois = old_line(root, "task", json!({"wave": 2, "text": "Fazer a parte dois.", "origin": said,
                "files": [{"path": "src/dois.rs"}], "covers": [two], "points": 3}));
            old.task.insert("dois", dois);
            old.wave.insert(3, hand_wave(root, said, 3, &[old.shared], &[1]));
            old.task.insert("tres_a", hand_task(root, said, 3, "Fazer a parte três.", Some("src/tres.rs"), &[old.shared]));
            old.task.insert("tres_b", hand_task(root, said, 3, "Documentar a parte três.", None, &[]));
            old.wave.insert(4, hand_wave(root, said, 4, &[old.shared], &[2, 3]));
            old.task.insert("quatro_a", hand_task(root, said, 4, "Juntar as partes.", Some("src/quatro_a.rs"), &[old.shared]));
            old.task.insert("quatro_b", hand_task(root, said, 4, "Mostrar o resultado.", Some("src/quatro_b.rs"), &[]));
        });
        delivered_and_committed(root, 1, "src/um.rs");
        old
    }

    /// A versão vigente de `id`.
    fn now(log: &SpecLog, id: u64) -> &SpecEvent {
        log.current(id).unwrap_or_else(|| panic!("o item {id} saiu da leitura"))
    }

    /// De uma tarefa: os arquivos, as tarefas de que ela depende, pelo texto,
    /// e o que ela cobre.
    type Row = (Vec<String>, BTreeSet<String>, Vec<u64>);

    /// O resumo de cada tarefa, pelo texto.
    fn summary(log: &SpecLog) -> BTreeMap<String, Row> {
        let text = |id: u64| now(log, id).str_field("text").unwrap_or_default().to_string();
        log.visible()
            .into_iter()
            .filter(|e| e.event_type == "task")
            .map(|t| {
                let on = t.ints("depends_on").into_iter().map(text).collect();
                (text(t.id), (declared_files(t), on, t.ints("covers")))
            })
            .collect()
    }

    /// Os removidos por remoção do programa, com o motivo de cada um.
    fn removed_by_binary(log: &SpecLog) -> Vec<(u64, String)> {
        log.visible()
            .into_iter()
            .filter(|e| e.event_type == "remove" && e.str_field("author") == Some("binary"))
            .flat_map(|e| e.ints("targets").into_iter().map(|t| (t, e.str_field("reason").unwrap_or_default().to_string())))
            .collect()
    }

    /// Confere a spec antiga já convertida, como a conversão inteira a deixa.
    fn assert_converted(root: &Path, old: &Old) {
        let log = log_of(root);
        let lang = crate::commands::spec_events::project(root).lang;

        // A onda 1 fica como história: a onda e a tarefa dela, intocadas.
        assert_eq!(now(&log, old.wave[&1]).id, old.wave[&1], "a onda 1 entregue fica");
        assert_eq!(now(&log, old.task["um"]).id, old.task["um"], "a tarefa da onda 1 fica");

        // As ondas 2, 3 e 4 ganham uma remoção do programa, com o motivo.
        let removed = removed_by_binary(&log);
        let mut targets: Vec<u64> = removed.iter().map(|(t, _)| *t).collect();
        targets.sort_unstable();
        assert_eq!(targets, vec![old.wave[&2], old.wave[&3], old.wave[&4]], "uma remoção por onda: {removed:?}");
        for (_, reason) in &removed {
            assert_eq!(reason, translate("wave.hand_drawn_removed", lang));
        }

        // Cada tarefa delas ganha uma versão sem onda e sem nota: a que
        // substitui a antiga. O lote que o backlog forma depois dá outra, com o
        // número dele.
        for name in ["dois", "tres_a", "tres_b", "quatro_a", "quatro_b"] {
            let versions: Vec<&SpecEvent> =
                log.events.iter().filter(|e| e.int("replaces") == Some(old.task[name])).collect();
            assert_eq!(versions.len(), 1, "{name}: uma versão nova só");
            let task = versions[0];
            assert!(!task.fields.contains_key("wave"), "{name}: sem número de onda: {:?}", task.fields);
            assert!(!task.fields.contains_key("points"), "{name}: sem a nota: {:?}", task.fields);
        }

        // A dependência entre ondas vira dependência entre tarefas; o
        // critério das ondas 3 e 4 fica numa tarefa só, a última da 4, que
        // depende das outras tarefas das duas ondas.
        let got = summary(&log);
        let set = |names: &[&str]| -> BTreeSet<String> {
            names.iter().map(|n| now(&log, old.task[n]).str_field("text").unwrap().to_string()).collect()
        };
        let row = |name: &str| got[now(&log, old.task[name]).str_field("text").unwrap()].clone();
        assert_eq!(row("dois").1, set(&["um"]));
        assert_eq!(row("tres_a").1, set(&["um"]));
        assert_eq!(row("tres_b").1, set(&["um"]));
        assert_eq!(row("quatro_a").1, set(&["dois", "tres_a", "tres_b"]));
        assert_eq!(row("quatro_b").1, set(&["dois", "tres_a", "tres_b", "quatro_a"]));
        let holders: Vec<&str> = ["dois", "tres_a", "tres_b", "quatro_a", "quatro_b"]
            .into_iter()
            .filter(|n| row(n).2.contains(&old.shared))
            .collect();
        assert_eq!(holders, vec!["quatro_b"], "o critério comum fica numa tarefa só");
        assert!(!row("dois").2.is_empty(), "o critério só da onda 2 fica com a tarefa dela");

        // A tarefa sem arquivo leva os da onda.
        assert_eq!(row("tres_b").0, vec!["src/tres.rs".to_string()]);
    }

    /// A rodada começa numa spec no formato da pi-kpis-plantio: a onda 1 fica
    /// como história, as ondas desenhadas à mão que nunca saíram saem da
    /// leitura com o motivo, as tarefas delas voltam para o backlog com a
    /// dependência entre ondas virada dependência entre tarefas, o critério
    /// comum fica numa tarefa só, e o primeiro lote sai com o número 2, sem
    /// número de onda repetido.
    #[test]
    fn o_primeiro_lote_depois_da_onda_1_entregue_e_a_onda_2() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let old = old_spec(root);

        let out = round(root, "x", None);
        assert_ne!(out["ok"], json!(false), "{out}");
        assert_converted(root, &old);

        let log = log_of(root);
        let mut numbers: Vec<u64> =
            log.visible().into_iter().filter(|e| e.event_type == "wave").filter_map(SpecEvent::wave).collect();
        numbers.sort_unstable();
        let unique: BTreeSet<u64> = numbers.iter().copied().collect();
        assert_eq!(numbers.len(), unique.len(), "nenhum número de onda se repete: {numbers:?}");
        assert_eq!(unique, BTreeSet::from([1, 2]), "a onda 1 e o primeiro lote: {numbers:?}");
        let lot = log
            .visible()
            .into_iter()
            .find(|e| e.event_type == "wave" && e.wave() == Some(2))
            .expect("o primeiro lote");
        assert_eq!(lot.str_field("author"), Some("binary"), "o lote 2 é do programa");
        let in_lot: BTreeSet<u64> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "task" && e.wave() == Some(2))
            .map(|t| t.id)
            .collect();
        let expected: BTreeSet<u64> = ["dois", "tres_a", "tres_b"].iter().map(|n| now(&log, old.task[n]).id).collect();
        assert_eq!(in_lot, expected, "o primeiro lote leva só as tarefas prontas: {out}");
    }

    /// Numa spec já convertida, a conversão não grava nada; numa em que ela
    /// parou no meio, com a onda já removida e as tarefas ainda apontando
    /// para ela, a rodada completa só o que faltava, e o resultado é o mesmo
    /// da conversão inteira.
    #[test]
    fn a_conversao_numa_spec_ja_convertida_nao_grava_nada() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let old = old_spec(root);
        round(root, "x", None);
        assert_converted(root, &old);
        let whole = summary(&log_of(root));

        let count = |root: &Path| {
            let log = log_of(root);
            ["remove", "task"].map(|t| log.events.iter().filter(|e| e.event_type == t).count())
        };
        let before = count(root);
        let lang = crate::commands::spec_events::project(root).lang;
        assert_eq!(convert_hand_waves(root, root, "x", lang), Ok(Vec::new()), "já convertida, nada a gravar");
        round(root, "x", None);
        assert_eq!(count(root), before, "a rodada seguinte não converte de novo");

        // A conversão que parou depois de remover a onda 3.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let old = old_spec(root);
        let draft = json!({"targets": [old.wave[&3]], "reason": translate("wave.hand_drawn_removed", lang),
            "author": "binary"});
        record(root, "x", "remove", draft.as_object().cloned().unwrap(), PhaseWriter::Binary).unwrap();
        round(root, "x", None);
        assert_converted(root, &old);
        let removals = removed_by_binary(&log_of(root)).into_iter().filter(|(t, _)| *t == old.wave[&3]).count();
        assert_eq!(removals, 1, "a onda 3 não é removida de novo");
        let rest: BTreeMap<_, _> = summary(&log_of(root));
        let without_lot = |s: &BTreeMap<String, Row>| {
            s.iter().map(|(k, (f, d, _))| (k.clone(), (f.clone(), d.clone()))).collect::<BTreeMap<_, _>>()
        };
        assert_eq!(without_lot(&rest), without_lot(&whole), "o mesmo resultado da conversão inteira");
    }

    /// A onda desenhada à mão que foi revista, com uma versão anterior, sai
    /// inteira na conversão: depois da rodada, nenhuma das duas versões está
    /// na leitura, as duas saíram pela remoção do programa, e a tarefa dela
    /// foi para o backlog, de onde o lote a leva.
    #[test]
    fn a_onda_a_mao_com_versao_anterior_sai_inteira_na_conversao() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/um.rs"), "fn um() {}\n").unwrap();
        let (mut first, mut second, mut task) = (0, 0, 0);
        approved_with(root, "x", &[], |said| {
            let crit = criterion(root, said, "a parte um passa");
            first = hand_wave(root, said, 1, &[crit], &[]);
            second = seed_event(root, "x", "wave", json!({"n": 1, "text": "Onda 1, à mão, revista.", "criteria": [crit],
                "done_when": "A suíte passa.", "depends_on": [], "origin": said, "author": "assistant", "replaces": first}));
            task = hand_task(root, said, 1, "Fazer a parte um.", Some("src/um.rs"), &[crit]);
        });
        let before = log_of(root);
        assert_eq!(before.current(first).map(|e| e.id), Some(second), "a versão revista é a vigente");

        let out = round(root, "x", None);
        assert_ne!(out["ok"], json!(false), "{out}");
        let log = log_of(root);
        let hidden = log.hidden();
        for (version, id) in [("anterior", first), ("revista", second)] {
            let by = match hidden.get(&id) {
                Some(Hidden::Removed { by }) => *by,
                other => panic!("a versão {version} da onda devia sair pela remoção: {other:?}"),
            };
            assert_eq!(log.get(by).and_then(|e| e.str_field("author")), Some("binary"), "a remoção é do programa");
        }
        let hand_drawn: Vec<u64> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "wave" && e.str_field("author") != Some("binary"))
            .map(|e| e.id)
            .collect();
        assert!(hand_drawn.is_empty(), "nenhuma versão da onda à mão fica na leitura: {hand_drawn:?}");
        let lot = now(&log, task).wave().expect("a tarefa está no lote");
        let lot_wave = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(lot));
        assert_eq!(lot_wave.and_then(|w| w.str_field("author")), Some("binary"), "o lote é do programa: {out}");
    }

    /// A onda desenhada à mão que já saiu e ainda roda termina como saiu, e a
    /// entregue ou aprovada fica como história: nenhuma tarefa delas vai para
    /// o backlog. Só a que nunca saiu é convertida.
    #[test]
    fn a_onda_que_ja_saiu_termina_como_saiu() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for file in ["src/um.rs", "src/dois.rs", "src/tres.rs", "src/quatro.rs"] {
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::write(root.join(file), "fn um() {}\n").unwrap();
        }
        let mut waves = BTreeMap::new();
        let mut tasks = BTreeMap::new();
        approved_with(root, "x", &[], |said| {
            let crit = criterion(root, said, "a parte passa");
            for (n, file) in [(1, "src/um.rs"), (2, "src/dois.rs"), (3, "src/tres.rs"), (4, "src/quatro.rs")] {
                waves.insert(n, hand_wave(root, said, n, &[crit], &[]));
                tasks.insert(n, hand_task(root, said, n, &format!("Fazer a parte {n}."), Some(file), &[crit]));
            }
        });
        // A 1 saiu e ainda roda; a 2 entregou; a 3 foi aprovada; a 4 nunca saiu.
        seed_send(root, 1);
        seed_event(root, "x", "delivered", json!({"wave": 2, "text": "Pronta.", "files": ["src/dois.rs"], "author": "wave"}));
        seed_event(root, "x", "verdict", json!({"wave": 3, "result": "approved", "final": true, "text": "Passou.",
            "author": "review", "criteria": []}));

        round(root, "x", None);
        let log = log_of(root);
        let removed: Vec<u64> = removed_by_binary(&log).into_iter().map(|(t, _)| t).collect();
        assert_eq!(removed, vec![waves[&4]], "só a onda que nunca saiu é removida");
        for n in [1, 2, 3] {
            assert_eq!(now(&log, waves[&n]).id, waves[&n], "a onda {n} fica como está");
            let task = now(&log, tasks[&n]);
            assert_eq!(task.id, tasks[&n], "a tarefa da onda {n} não ganha versão nova");
            assert_eq!(task.wave(), Some(n), "a tarefa da onda {n} não vai para o backlog");
        }
        let four = now(&log, tasks[&4]);
        assert_ne!(four.id, tasks[&4], "a tarefa da onda que nunca saiu volta para o backlog");
    }

    /// A regra que diz em `waves` uma onda que a conversão remove ganha a
    /// versão nova sem `waves`, com o dono pelos arquivos das tarefas daquela
    /// onda; a rodada segue sem pedir a análise, e a regra vai no pedido do
    /// lote que toca esses arquivos.
    #[test]
    fn o_item_com_dono_pelos_arquivos_vai_no_pedido_da_onda_que_toca_neles_depois_da_conversao() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let old = old_spec(root);
        let said = seed_event(root, "x", "message", json!({"author": "user", "text": "a três segue a um"}));
        let rule = seed_event(root, "x", "rule", json!({"text": "A parte três segue o molde da parte um.",
            "keys": ["molde"], "example": "o mesmo formato da um", "waves": [3], "origin": said}));

        let out = round(root, "x", None);
        assert_ne!(out["ok"], json!(false), "a rodada não trava: {out}");
        assert_converted(root, &old);
        let log = log_of(root);
        let version = now(&log, rule);
        assert_ne!(version.id, rule, "a regra ganha versão nova");
        assert!(!version.fields.contains_key("waves"), "sem a onda removida: {:?}", version.fields);
        assert_eq!(version.fields.get("applies_to"), Some(&json!({"files": ["src/tres.rs"]})), "{:?}", version.fields);
        assert_eq!(version.str_field("text"), Some("A parte três segue o molde da parte um."));

        assert!(out.get("analysis").is_none(), "a regra com dono não pede a análise: {out}");
        let code = log.codes().get(&version.id).cloned().expect("o código da regra");
        let prompt = out["dispatch"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["wave"] == json!(2))
            .and_then(|d| d["prompt"].as_str())
            .unwrap_or_else(|| panic!("o lote 2 sai: {out}"))
            .to_string();
        assert!(prompt.contains(&code), "o lote que toca src/tres.rs leva a regra: {prompt}");
    }
}
