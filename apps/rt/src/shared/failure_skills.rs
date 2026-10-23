//! A falha que se repete vira skill. Cada reprovação de onda com motivo vira
//! uma linha no registro das falhas do projeto ([`mustard_core::io::failures`]):
//! o veredito reprovado do revisor de onda, a linha `REJECTED` de quem conduz
//! a obra e o veredito reprovado da revisão final que aponta uma onda. As
//! linhas saem do arquivo de eventos da spec, depois da rodada, fora das
//! travas dela ([`after_round`]); a mesma reprovação nunca entra duas vezes.
//!
//! A linha nova ganha o grupo da mesma falha: as anteriores que dividem com
//! ela um arquivo ou uma pasta vão ao Jev, uma pergunta de sim ou não por
//! candidata, e o sim de [`SURE`] para cima a põe no grupo da candidata. Sem
//! Jev ou na dúvida, ela fica no grupo dela. O Jev ligado que não responde
//! manda a pergunta à reserva dele ([`crate::shared::jev::reserve`]): a falha
//! fica sem grupo até o recibo, que decide no lugar do Jev. O grupo
//! com duas ocorrências e sem tarefa ganha, na spec aberta da segunda, a
//! tarefa de criar a skill que a evita, com o skill-creator inteiro no bloco
//! dela; sem spec que rode, a tarefa espera no registro.
//!
//! A skill que a tarefa cria é gravada pelo `run write skill`
//! ([`prepare_skill`], [`after_skill_written`]): a pasta vem para o projeto
//! principal, fica fora do git pela regra local do clone, ganha a cópia de
//! segurança na pasta do Mustard fora do projeto e entra na linha da spec no
//! resumo do projeto.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::project_summary::SURE;
use mustard_core::domain::spec_events::{SpecEvent, SpecLog, TASK_TITLE_MAX};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState as _};
use mustard_core::domain::text::cut_line;
use mustard_core::io::failures::{self as disk, Failure, SkillTask};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use crate::shared::jev::yes_no::{self, YesNo};
use crate::shared::secret::without_secrets;
use crate::shared::spec_state::DiskSpecState;

/// O nome que a pergunta da mesma falha grava.
pub(crate) const FAILURES_CALL: &str = "same failure";

/// Quantos caracteres do motivo de uma falha vão ao Jev.
const REASON_CHARS: usize = 1_000;

/// Quantos caracteres do motivo de cada ocorrência vão à tarefa.
const OCCURRENCE_CHARS: usize = 300;

/// Quantas ocorrências, as mais recentes, a tarefa lista.
const OCCURRENCES_MAX: usize = 6;

/// Quantos arquivos das ocorrências a tarefa declara.
const TASK_FILES_MAX: usize = 8;

/// Quantos caracteres do motivo da primeira ocorrência entram no título.
const TITLE_REASON_CHARS: usize = 60;

/// Os caminhos de `value`, uma lista de textos ou de objetos com `path`.
fn paths_in(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| file.as_str().or_else(|| file.get("path").and_then(Value::as_str)))
        .map(str::to_string)
        .collect()
}

/// Os arquivos de `files` da onda `wave` relativos ao repositório: o caminho
/// absoluto dentro da cópia gravada no envio dela perde a cópia.
fn relative(log: &SpecLog, wave: u64, files: Vec<String>) -> Vec<String> {
    let copy = mustard_core::io::wave_prompt::recorded_copy(log, wave);
    let mut out: Vec<String> = Vec::new();
    for file in files {
        let file = copy.as_ref().and_then(|copy| crate::shared::paths::below(&copy.path, &file)).unwrap_or(file);
        if !file.is_empty() && !out.contains(&file) {
            out.push(file);
        }
    }
    out
}

/// Os arquivos da última entrega da onda `wave` em `log`.
fn delivered_files(log: &SpecLog, wave: u64) -> Vec<String> {
    log.events.iter().rev().find(|e| e.event_type == "delivered" && e.wave() == Some(wave)).map(|e| paths_in(e.fields.get("files"))).unwrap_or_default()
}

/// A falha `id` da spec `spec`, da onda `wave`, em `event`.
fn failure(spec: &str, event: &SpecEvent, wave: u64, source: &str, reason: &str, files: Vec<String>) -> Failure {
    Failure {
        id: format!("{spec}:{}", event.id),
        spec: spec.to_string(),
        wave,
        at: event.at().to_string(),
        source: source.to_string(),
        reason: without_secrets(reason.trim()),
        files,
        ..Failure::default()
    }
}

/// As reprovações de onda com motivo da spec `spec` em `log`, na ordem: cada
/// volta reprovada uma vez só — pelo revisor da onda, quando o veredito dele
/// a mandou de volta, senão por quem conduz a obra —, e cada veredito
/// reprovado da revisão final que aponta uma onda.
pub(crate) fn from_log(spec: &str, log: &SpecLog) -> Vec<Failure> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<(u64, u64)> = BTreeSet::new();
    for send in log.events.iter().filter(|e| e.event_type == "send") {
        let (Some(wave), Some(rejected)) = (send.wave(), send.fields.get("rejected")) else { continue };
        let Some(back) = rejected.get("delivered").and_then(Value::as_u64) else { continue };
        if !seen.insert((wave, back)) {
            continue;
        }
        let returned = log.get(back).map(|e| paths_in(e.fields.get("files"))).unwrap_or_default();
        let review = log.events.iter().rev().find(|e| {
            e.event_type == "verdict"
                && e.wave() == Some(wave)
                && (back..send.id).contains(&e.id)
                && !e.returned()
                && e.str_field("result") == Some("rejected")
                && e.fields.get("wave_review") == Some(&Value::Bool(true))
        });
        let row = match review {
            Some(verdict) => {
                let files = paths_in(verdict.fields.get("files"));
                let files = relative(log, wave, if files.is_empty() { returned } else { files });
                failure(spec, send, wave, "wave_review", verdict.str_field("text").unwrap_or_default(), files)
            }
            None => failure(spec, send, wave, "conductor", rejected.get("reason").and_then(Value::as_str).unwrap_or_default(), relative(log, wave, returned)),
        };
        out.push(row);
    }
    for verdict in log.events.iter().filter(|e| {
        e.event_type == "verdict"
            && !e.returned()
            && e.str_field("author") == Some("review")
            && e.fields.get("final") == Some(&Value::Bool(true))
            && e.str_field("result") == Some("rejected")
    }) {
        let Some(wave) = verdict.wave() else { continue };
        let files = paths_in(verdict.fields.get("files"));
        let files = relative(log, wave, if files.is_empty() { delivered_files(log, wave) } else { files });
        out.push(failure(spec, verdict, wave, "final_review", verdict.str_field("text").unwrap_or_default(), files));
    }
    out.retain(|row| !row.reason.is_empty());
    out
}

/// O que o Jev vê de uma falha: o motivo, cortado, e os arquivos.
fn shown(failure: &Failure) -> Value {
    json!({ "reason": without_secrets(&cut_line(&failure.reason, REASON_CHARS)), "files": failure.files })
}

/// A pergunta da mesma falha: a nova no estado, cada candidata uma entrada.
fn same_question(new: &Failure) -> YesNo<'static> {
    let mut context = Map::new();
    context.insert("failures".to_string(), json!({ "new": shown(new) }));
    YesNo {
        context,
        section: "failures",
        question: "Is `failures.new` the same problem as `{entry}`?",
        focus: "Compare the underlying mistake behind each failure. Touching the same file or folder alone is not the same problem.",
        yes: "Both failures come from the same underlying mistake, so one lesson would prevent both",
        no: "The failures only share a file or folder; the underlying mistakes differ",
    }
}

/// O grupo em que a falha `new` entra, pela resposta do Jev `jev` (`None`
/// quando desligado) sobre as candidatas dela em `all`, com as chamadas
/// gravadas na spec `spec` de `root`: o da candidata de maior chance, de
/// [`SURE`] para cima. `Ok(None)` sem candidata, sem resposta ou na dúvida;
/// `Err` com o pedido de reserva quando o Jev ligado não respondeu e a
/// decisão espera o recibo dela.
fn same_group(root: &Path, spec: &str, all: &[Failure], new: &Failure, jev: Option<&yes_no::AskFn<'_>>) -> Result<Option<String>, Value> {
    let candidates = disk::candidates(all, new);
    if candidates.is_empty() {
        return Ok(None);
    }
    let key = |at: usize| format!("f{}", at + 1);
    let entries: Vec<(String, Value)> = candidates.iter().enumerate().map(|(at, row)| (key(at), shown(row))).collect();
    let chances = match yes_no::ask_all_reserved(root, spec, FAILURES_CALL, &[(&same_question(new), &entries)], jev) {
        yes_no::Decided::Chances(got) => got.into_iter().next().flatten(),
        yes_no::Decided::Off => None,
        yes_no::Decided::Reserve(request) => return Err(request),
    };
    let Some(chances) = chances else { return Ok(None) };
    Ok(candidates
        .iter()
        .enumerate()
        .filter_map(|(at, row)| chances.get(&key(at)).filter(|chance| **chance >= SURE).map(|chance| (*chance, row)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, row)| row.group_id().to_string()))
}

/// Dá o grupo a cada falha do projeto `root` que ainda não tem, na ordem do
/// registro, com o Jev `jev` (`None` quando desligado) e as chamadas
/// gravadas na spec `spec`. Devolve as que ganharam grupo, pelo `id`, e o
/// pedido de reserva da primeira que o Jev ligado não agrupou: ela e as
/// seguintes ficam sem grupo até o recibo.
///
/// # Errors
///
/// A falha de gravação do registro, com o motivo.
fn group_pending_with(root: &Path, spec: &str, jev: Option<&yes_no::AskFn<'_>>) -> Result<(Vec<String>, Option<Value>), String> {
    let pending: Vec<String> = disk::read(root).into_iter().filter(|row| row.group.is_none()).map(|row| row.id).collect();
    let mut grouped = Vec::new();
    for id in &pending {
        let all = disk::read(root);
        let Some(new) = all.iter().find(|row| &row.id == id && row.group.is_none()) else { continue };
        let group = match same_group(root, spec, &all, new, jev) {
            Ok(group) => group.unwrap_or_else(|| new.id.clone()),
            Err(request) => return Ok((grouped, Some(request))),
        };
        disk::change(root, |rows| {
            for row in rows.iter_mut().filter(|row| &row.id == id && row.group.is_none()) {
                row.group = Some(group.clone());
            }
        })
        .map_err(|error| error.to_string())?;
        grouped.push(id.clone());
    }
    Ok((grouped, None))
}

/// A spec `spec` de `start` pode receber a tarefa agora: ela roda.
pub(crate) fn runs(start: &Path, spec: &str) -> bool {
    DiskSpecState::new(start).state(spec).and_then(|state| state.phase).is_some_and(crate::commands::flow::round::can_run)
}

/// A spec aberta onde nasce a tarefa do grupo cuja última ocorrência é de
/// `spec`: ela mesma, quando roda, senão a spec atual do checkout de `start`,
/// quando roda.
pub(crate) fn target_spec(start: &Path, spec: &str) -> Option<String> {
    if runs(start, spec) {
        return Some(spec.to_string());
    }
    let checkout = crate::shared::spec_state::checkout(start);
    crate::shared::spec_state::active_spec(&checkout.to_string_lossy(), None).filter(|active| runs(start, active))
}

/// O nome de quem reprovou, no idioma `lang`.
fn source_name(source: &str, lang: Locale) -> String {
    translate(&format!("failures.source.{source}"), lang).to_string()
}

/// A tarefa de criar a skill do grupo de ocorrências `members`, no idioma
/// `lang`: o título com o motivo da primeira, as ocorrências na parte do
/// agente, o skill-creator nomeado e os arquivos delas.
fn skill_task_draft(members: &[&Failure], lang: Locale) -> Map<String, Value> {
    let count = members.len().to_string();
    let title = translate("failures.skill_task_title", lang);
    let room = TASK_TITLE_MAX.saturating_sub(title.replace("{reason}", "").chars().count()).min(TITLE_REASON_CHARS);
    let title = title.replace("{reason}", &cut_line(&members[0].reason, room));
    let recent = &members[members.len().saturating_sub(OCCURRENCES_MAX)..];
    let occurrences: Vec<String> = recent
        .iter()
        .map(|row| {
            let mut line = translate("failures.occurrence", lang)
                .replace("{spec}", &row.spec)
                .replace("{wave}", &row.wave.to_string())
                .replace("{source}", &source_name(&row.source, lang))
                .replace("{reason}", &cut_line(&row.reason, OCCURRENCE_CHARS));
            if !row.files.is_empty() {
                let files: Vec<String> = row.files.iter().map(|file| format!("`{file}`")).collect();
                line.push(' ');
                line.push_str(&translate("failures.occurrence_files", lang).replace("{files}", &files.join(", ")));
            }
            line
        })
        .collect();
    let mut files: Vec<String> = Vec::new();
    for file in members.iter().flat_map(|row| row.files.iter()) {
        if files.len() < TASK_FILES_MAX && !files.contains(file) {
            files.push(file.clone());
        }
    }
    let draft = json!({
        "title": title,
        "text": translate("failures.skill_task_text", lang).replace("{count}", &count),
        "agent": translate("failures.skill_task_agent", lang).replace("{count}", &count).replace("{occurrences}", &occurrences.join("\n")),
        "skill": mustard_core::io::skill_creator::NAME,
        "files": files.iter().map(|path| json!({ "path": path })).collect::<Vec<_>>(),
        "depends_on": [],
        "author": "binary",
    });
    draft.as_object().cloned().unwrap_or_default()
}

/// Cria a tarefa de criar a skill de cada grupo do projeto de `start` com
/// duas ocorrências ou mais e sem tarefa, na spec aberta da última
/// ([`target_spec`]), e marca no grupo que ela existe. Sem spec que rode, o
/// grupo fica marcado à espera. Devolve as tarefas gravadas.
///
/// # Errors
///
/// A falha de gravação, com o motivo.
fn skill_tasks(start: &Path, lang: Locale) -> Result<Vec<Value>, String> {
    let root = store::spec_root(start);
    let all = disk::read(&root);
    let mut groups: Vec<&str> = Vec::new();
    for row in &all {
        if let Some(group) = row.group.as_deref()
            && !groups.contains(&group)
        {
            groups.push(group);
        }
    }
    let mut recorded = Vec::new();
    for group in groups {
        let members = disk::members(&all, group);
        let done = members.iter().any(|row| row.skill_task.as_ref().is_some_and(|task| task.task.is_some()));
        let (Some(last), false) = (members.last(), done || members.len() < 2) else { continue };
        let mark = match target_spec(start, &last.spec) {
            Some(spec) => {
                let written = crate::commands::spec_events::write::record(start, &spec, "task", skill_task_draft(&members, lang), PhaseWriter::Binary)
                    .map_err(|refusal| refusal.message(lang))?;
                recorded.push(json!({ "type": "task", "spec": spec, "id": written.written.id, "skill": mustard_core::io::skill_creator::NAME, "group": group }));
                SkillTask { spec, task: Some(written.written.id) }
            }
            None if members.iter().any(|row| row.skill_task.is_some()) => continue,
            None => SkillTask::default(),
        };
        let (group, last) = (group.to_string(), last.id.clone());
        disk::change(&root, |rows| {
            for row in rows.iter_mut().filter(|row| row.group.as_deref() == Some(group.as_str())) {
                row.skill_task = (row.id == last).then(|| mark.clone());
            }
        })
        .map_err(|error| error.to_string())?;
    }
    Ok(recorded)
}

/// O que a passagem do registro das falhas deixa: as tarefas gravadas, os
/// avisos e os pedidos de reserva das falhas que esperam o grupo.
#[derive(Debug, Default)]
pub(crate) struct Settled {
    pub(crate) recorded: Vec<Value>,
    pub(crate) warnings: Vec<String>,
    pub(crate) reserves: Vec<Value>,
}

/// A passagem do registro das falhas do projeto de `start`, fora de qualquer
/// trava da rodada: o grupo de cada falha que espera, com o Jev do projeto, e
/// a tarefa de cada grupo que se repetiu. Devolve as tarefas gravadas, os
/// avisos, em `lang`, e o pedido de reserva da falha que o Jev ligado não
/// agrupou: ela fica sem grupo até o recibo.
pub(crate) fn settle(start: &Path, spec: &str, lang: Locale) -> Settled {
    let root = store::spec_root(start);
    if !disk::path(&root).is_file() {
        return Settled::default();
    }
    let waiting = disk::read(&root).iter().any(|row| row.group.is_none());
    let jev = if waiting { yes_no::project_jev(&root, spec, FAILURES_CALL) } else { None };
    settle_with(start, spec, jev.as_deref(), lang)
}

/// [`settle`] com o Jev `jev`, com as chamadas gravadas na spec `spec`. A
/// falha que caiu no grupo que motivou uma skill do Mustard, numa onda que a
/// recebeu, pede a atualização dela
/// ([`crate::shared::skill_cycle::after_failures`]).
fn settle_with(start: &Path, spec: &str, jev: Option<&yes_no::AskFn<'_>>, lang: Locale) -> Settled {
    let failed = |detail: String| translate("failures.not_recorded", lang).replace("{detail}", &detail);
    let (grouped, reserve) = match group_pending_with(&store::spec_root(start), spec, jev) {
        Ok(grouped) => grouped,
        Err(detail) => return Settled { warnings: vec![failed(detail)], ..Settled::default() },
    };
    let (mut recorded, mut warnings) = match skill_tasks(start, lang) {
        Ok(recorded) => (recorded, Vec::new()),
        Err(detail) => (Vec::new(), vec![failed(detail)]),
    };
    match crate::shared::skill_cycle::after_failures(start, &grouped, lang) {
        Ok(updates) => recorded.extend(updates),
        Err(detail) => warnings.push(failed(detail)),
    }
    Settled { recorded, warnings, reserves: reserve.into_iter().collect() }
}

/// Depois da rodada da spec `spec` de `start`, com as travas dela soltas: as
/// reprovações novas entram no registro, e a passagem ([`settle`]) as agrupa
/// e cria as tarefas. Melhor esforço: a falha vira aviso, em `lang`.
pub(crate) fn after_round(start: &Path, spec: &str, lang: Locale) -> Settled {
    let root = store::spec_root(start);
    let Some(log) = store::spec_file(&root, spec).ok().and_then(|file| store::read(&file).ok().flatten()) else {
        return Settled::default();
    };
    let found = from_log(spec, &log);
    if found.is_empty() && !disk::path(&root).is_file() {
        return Settled::default();
    }
    if let Err(error) = disk::append(&root, &found) {
        return Settled { warnings: vec![translate("failures.not_recorded", lang).replace("{detail}", &error.to_string())], ..Settled::default() };
    }
    settle(start, spec, lang)
}

/// [`after_round`] depois da rodada que terminou em `done`, que roda sempre,
/// também na recusa: na resposta da rodada, as tarefas gravadas entram em
/// `recorded` e a falha do registro entra nos avisos.
///
/// # Errors
///
/// A recusa da rodada, como veio.
pub(crate) fn after_round_into<E>(start: &Path, spec: &str, lang: Locale, done: Result<Value, E>) -> Result<Value, E> {
    let Settled { recorded: mut tasks, warnings, mut reserves } = after_round(start, spec, lang);
    let (skills, said, stale) = crate::shared::skill_cycle::after_round(start, spec, lang);
    tasks.extend(skills);
    reserves.extend(stale);
    let mut out = done?;
    crate::shared::jev::reserve::attach(&mut out, reserves, "mustard-rt run round", lang);
    for hint in warnings {
        crate::commands::spec_events::pages::push_warning(&mut out, "failures-not-recorded", &hint);
    }
    for (reason, hint) in said {
        crate::commands::spec_events::pages::push_warning(&mut out, reason, &hint);
    }
    if !tasks.is_empty() {
        match out.get_mut("recorded").and_then(Value::as_array_mut) {
            Some(recorded) => recorded.extend(tasks),
            None => out["recorded"] = json!(tasks),
        }
    }
    Ok(out)
}

/// O nome da skill serve de pasta: letras, números, hífen e sublinhado.
fn good_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A pasta da skill `name` no projeto `root`.
pub(crate) fn skill_dir(root: &Path, name: &str) -> PathBuf {
    root.join(".claude").join("skills").join(name)
}

/// Copia a pasta `from` inteira para `to`, no lugar do que havia lá.
pub(crate) fn copy_folder(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.exists() {
        std::fs::remove_dir_all(to)?;
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// A pasta da skill `name` que a onda escreveu fora do projeto principal
/// `root`: a de `start`, quando o comando roda de dentro da cópia, e a da
/// cópia de cada onda da spec `spec`, a mais recente primeiro.
fn written_in_copy(start: &Path, root: &Path, spec: &str, name: &str) -> Option<PathBuf> {
    let mut places = vec![skill_dir(start, name)];
    if let Some(log) = store::spec_file(root, spec).ok().and_then(|file| store::read(&file).ok().flatten()) {
        places.extend(log.events.iter().rev().filter(|e| e.event_type == "send").filter_map(|e| e.str_field("copy")).map(|copy| skill_dir(Path::new(copy), name)));
    }
    let main = skill_dir(root, name);
    let same = |dir: &Path| *dir == main || matches!((dir.canonicalize(), main.canonicalize()), (Ok(a), Ok(b)) if a == b);
    places.into_iter().find(|dir| !same(dir) && dir.join("SKILL.md").is_file())
}

/// A impressão do `SKILL.md` da pasta `dir`.
fn skill_sha(dir: &Path) -> Option<String> {
    let text = std::fs::read(dir.join("SKILL.md")).ok()?;
    let mut hash = mustard_core::io::sha256::Sha256::new();
    hash.update(&text);
    Some(hash.hex_digest())
}

/// Antes de gravar o `skill` criado ou atualizado (`draft`) na spec `spec` de
/// `start`: a pasta da skill vem da cópia da onda para o projeto principal,
/// e a impressão do arquivo entra no evento. A recusa, pronta para a
/// resposta em `lang`, quando o nome não serve de pasta ou a skill não está
/// em lugar nenhum.
pub(crate) fn prepare_skill(start: &Path, spec: &str, draft: &mut Map<String, Value>, lang: Locale) -> Result<(), Value> {
    let name = draft.get("name").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();
    if !good_name(&name) {
        return Err(json!({ "ok": false, "reason": "skill-bad-name", "hint": translate("skills.bad_name", lang).replace("{name}", &name) }));
    }
    if !matches!(draft.get("action").and_then(Value::as_str).map(str::trim), Some("created" | "updated")) {
        return Ok(());
    }
    let root = store::spec_root(start);
    let main = skill_dir(&root, &name);
    if let Some(written) = written_in_copy(start, &root, spec, &name) {
        copy_folder(&written, &main).map_err(|error| json!({ "ok": false, "reason": "skill-not-copied", "hint": error.to_string() }))?;
    }
    let Some(sha) = skill_sha(&main) else {
        return Err(json!({ "ok": false, "reason": "skill-folder-missing", "hint": translate("skills.folder_missing", lang).replace("{name}", &name) }));
    };
    draft.insert("sha".into(), json!(sha));
    Ok(())
}

/// A regra local do clone que esconde a pasta da skill `name` do projeto
/// `root`, relativa à raiz do repositório.
fn skill_rule(root: &Path, name: &str) -> String {
    let prefix = mustard_core::platform::git::run(root, &["rev-parse", "--show-prefix"]).out().unwrap_or_default();
    format!("{}.claude/skills/{name}/", prefix.trim())
}

/// As skills que a spec criou ou atualizou e não descartou depois, em
/// `log`, na ordem.
pub(crate) fn skills_of(log: &SpecLog) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for skill in log.visible().into_iter().filter(|e| e.event_type == "skill") {
        let Some(name) = skill.str_field("name").map(str::trim) else { continue };
        out.retain(|had| had != name);
        if matches!(skill.str_field("action"), Some("created" | "updated")) {
            out.push(name.to_string());
        }
    }
    out
}

/// Depois de gravar o `skill` número `id`, de nome `name` e ação `action`, na
/// spec `spec` de `start`: o registro das skills do Mustard o guarda
/// ([`crate::shared::skill_cycle::skill_recorded`]); criado ou atualizado, a
/// pasta dele fica fora do git pela regra local do clone, a cópia de
/// segurança vai à pasta do Mustard fora do projeto e a linha da spec no
/// resumo do projeto, quando existe, passa a citá-lo. Só a pasta da skill que
/// o Mustard criou ganha a regra. Devolve os avisos, em `lang`.
pub(crate) fn after_skill_written(start: &Path, spec: &str, (id, name, action): (u64, &str, &str), lang: Locale) -> Vec<String> {
    let mut warnings = crate::shared::skill_cycle::skill_recorded(start, spec, id, lang);
    if !matches!(action, "created" | "updated") || !good_name(name) {
        return warnings;
    }
    let root = store::spec_root(start);
    let outcome = mustard_core::ensure_excluded(&root, &[skill_rule(&root, name)]);
    if let Some(failure) = outcome.unavailable {
        warnings.push(translate("skills.not_hidden", lang).replace("{name}", name).replace("{detail}", failure.reason()));
    }
    let backup = mustard_core::io::wave_prompt::skills_backup_dir(&root).join(name);
    if let Err(error) = copy_folder(&skill_dir(&root, name), &backup) {
        warnings.push(translate("skills.not_backed_up", lang).replace("{name}", name).replace("{detail}", &error.to_string()));
    }
    if let Some(log) = DiskSpecState::new(start).log(spec)
        && let Some(mut entry) = mustard_core::io::project_summary::read(&root).into_iter().find(|entry| entry.spec == spec)
    {
        entry.skills = skills_of(&log);
        if let Err(error) = mustard_core::io::project_summary::upsert(&root, &entry) {
            warnings.push(translate("spec_index.summary_failed", lang).replace("{spec}", spec).replace("{detail}", &error.to_string()));
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::process::Command;
    use std::rc::Rc;

    use super::*;

    fn row(id: &str, files: &[&str], group: Option<&str>) -> Failure {
        Failure {
            id: id.to_string(),
            spec: "x".into(),
            wave: 1,
            at: "2026-10-20T10:00:00Z".into(),
            source: "conductor".into(),
            reason: format!("O motivo de {id}."),
            files: files.iter().map(|file| (*file).to_string()).collect(),
            group: group.map(str::to_string),
            ..Failure::default()
        }
    }

    /// A Jev answering `chance` to every question, keeping each request.
    fn jev(chance: f64, asked: &Rc<RefCell<Vec<Value>>>) -> impl Fn(&[String]) -> Option<Vec<Value>> {
        let asked = Rc::clone(asked);
        move |payloads| {
            let docs = payloads.iter().map(|payload| {
                let body: Value = serde_json::from_str(payload).unwrap();
                let answers: Map<String, Value> = body["questions"].as_object().unwrap().keys().map(|key| (key.clone(), json!({"type":"noul","noul":chance}))).collect();
                asked.borrow_mut().push(body);
                json!({ "answers": answers })
            });
            Some(docs.collect())
        }
    }

    #[test]
    fn only_a_candidate_sharing_a_file_goes_to_the_jev_and_a_yes_joins_its_group_while_doubt_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        disk::append(root, &[row("a:1", &["src/a.rs"], Some("a:1")), row("a:2", &["docs/b.md"], Some("a:2")), row("x:3", &["src/a.rs"], None)]).unwrap();
        let asked = Rc::new(RefCell::new(Vec::new()));
        assert_eq!(group_pending_with(root, "", Some(&jev(0.92, &asked))).map(|(ids, _)| ids.len()), Ok(1));
        let state = asked.borrow()[0]["state"]["failures"].clone();
        let mut keys: Vec<&String> = state.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["f1", "new"], "only the candidate on the same file: {state}");
        assert_eq!(state["f1"]["reason"], "O motivo de a:1.");
        assert_eq!(disk::read(root)[2].group.as_deref(), Some("a:1"));

        disk::append(root, &[row("x:4", &["src/c.rs"], None)]).unwrap();
        assert_eq!(group_pending_with(root, "", Some(&jev(0.6, &asked))).map(|(ids, _)| ids.len()), Ok(1));
        assert_eq!(disk::read(root)[3].group.as_deref(), Some("x:4"), "doubt keeps its own group");
    }

    /// O Jev ligado que falha na pergunta da mesma falha manda a decisão à
    /// reserva: a passagem traz o pedido, com a mesma pergunta, e a falha
    /// fica sem grupo até o recibo. O recibo certo do sim a põe no grupo da
    /// candidata, sem chamar o Jev, e o grupo de duas ocorrências pede a
    /// tarefa da skill; o incerto vale como dúvida: grupo próprio. Com o Jev
    /// desligado, grupo próprio, sem reserva.
    #[test]
    fn a_failed_jev_sends_the_same_failure_question_to_the_reserve_and_the_receipt_groups() {
        for certainty in ["sure", "unsure"] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            disk::append(root, &[row("a:1", &["src/a.rs"], Some("a:1")), row("x:3", &["src/a.rs"], None)]).unwrap();
            yes_no::ask_in_test(Some(Rc::new(|_: &[String]| None)));
            let settled = settle(root, "", Locale::PtBr);
            assert_eq!(settled.reserves.len(), 1, "{settled:?}");
            let request = settled.reserves[0].clone();
            let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap();
            assert!(prompt.contains("Is `failures.new` the same problem as `failures.f1`?") && prompt.contains("O motivo de x:3."), "{prompt}");
            assert_eq!(disk::read(root)[1].group, None, "pending until the receipt");

            crate::shared::jev::reserve::answer_in_test(root, &request, |_| "yes".into(), certainty);
            yes_no::ask_in_test(Some(Rc::new(|_: &[String]| panic!("the receipt answers in place of the Jev"))));
            let applied = settle(root, "", Locale::PtBr);
            yes_no::ask_in_test(None);
            assert!(applied.reserves.is_empty(), "{applied:?}");
            let rows = disk::read(root);
            if certainty == "sure" {
                assert_eq!(rows[1].group.as_deref(), Some("a:1"));
                assert!(rows[1].skill_task.is_some(), "two occurrences ask for the skill task");
            } else {
                assert_eq!(rows[1].group.as_deref(), Some("x:3"), "an unsure receipt is a doubt");
            }
            disk::append(root, &[row("x:4", &["src/a.rs"], None)]).unwrap();
            assert!(settle(root, "", Locale::PtBr).reserves.is_empty(), "with the Jev off nothing goes to the reserve");
            assert_eq!(disk::read(root)[2].group.as_deref(), Some("x:4"));
        }
    }

    #[test]
    fn a_final_review_rejecting_a_wave_is_a_failure_and_a_machine_mark_is_not() {
        let lines = [
            json!({"type":"delivered","wave":2,"text":"Pronta.","files":["src/b.rs"],"author":"binary"}),
            json!({"type":"verdict","final":true,"wave":2,"result":"rejected","text":"A onda 2 repete a 1.","author":"review"}),
            json!({"type":"verdict","final":true,"wave":2,"result":"rejected","text":"Marca da máquina.","author":"binary"}),
            json!({"type":"verdict","final":true,"wave":2,"result":"rejected","text":"A volta do revisor.","author":"review","returned":true}),
        ];
        let text: String = lines
            .iter()
            .enumerate()
            .map(|(at, line)| {
                let mut line = line.clone();
                (line["v"], line["id"], line["at"]) = (json!(1), json!(at + 1), json!("2026-10-20T10:00:00Z"));
                line.to_string() + "\n"
            })
            .collect();
        let found = from_log("x", &mustard_core::domain::spec_events::parse_log(&text));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!((found[0].id.as_str(), found[0].source.as_str(), found[0].wave), ("x:2", "final_review", 2));
        assert_eq!((found[0].reason.as_str(), found[0].files.clone()), ("A onda 2 repete a 1.", vec!["src/b.rs".to_string()]));
    }

    fn git(root: &Path, args: &[&str]) -> bool {
        Command::new("git").args(["-c", "user.email=t@t", "-c", "user.name=t"]).args(args).current_dir(root).output().unwrap().status.success()
    }

    #[test]
    fn a_created_skill_comes_from_the_wave_copy_hidden_from_git_backed_up_and_in_the_project_summary() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::shared::test_fixture::seeded_repo(root, &[("mustard.json", "{}"), ("src/a.rs", "fn a() {}\n")]);
        assert!(git(root, &["checkout", "-q", "-B", "feature/x"]));
        assert_eq!(crate::commands::spec_events::write::record_open(root, "x", "feature/x", "dev"), Ok(true));
        let copies = tempfile::tempdir().unwrap();
        let copy = copies.path().join("a");
        assert!(git(root, &["worktree", "add", "-q", &copy.to_string_lossy(), "-b", "onda-1"]));
        let text = "---\nname: falta-teste\ndescription: Lembra o teste do caso vazio.\npaths:\n  - src/**\n---\n\nCorpo.\n";
        for (at, name, body) in [(copy.as_path(), "falta-teste", text), (root, "minha", "# Minha\n")] {
            std::fs::create_dir_all(skill_dir(at, name)).unwrap();
            std::fs::write(skill_dir(at, name).join("SKILL.md"), body).unwrap();
        }
        let body = json!({"name":"falta-teste","action":"created","text":"A onda deixou de esquecer o teste do vazio.","files":["src/a.rs"]});
        let opts = crate::commands::spec_events::write::WriteOpts { root: copy.clone(), spec: Some("x".into()), event_type: "skill".into(), json: body.to_string() };
        let out = crate::commands::spec_events::write::write_at(&opts);
        let backup = mustard_core::io::wave_prompt::skills_backup_dir(root);
        let backed_up = backup.join("falta-teste/SKILL.md").is_file();
        let _ = std::fs::remove_dir_all(&backup);
        assert_eq!(out["ok"], true, "{out}");
        assert_eq!(std::fs::read_to_string(skill_dir(root, "falta-teste").join("SKILL.md")).unwrap(), text, "the skill reached the main project");
        assert!(git(root, &["check-ignore", "-q", ".claude/skills/falta-teste/SKILL.md"]), "the skill Mustard created is hidden from git");
        assert!(!git(root, &["check-ignore", "-q", ".claude/skills/minha/SKILL.md"]), "the person's own skill is left alone");
        assert!(backed_up, "the backup copy outside the project");
        let log = DiskSpecState::new(root).log("x").unwrap();
        let skill = log.visible().into_iter().rfind(|e| e.event_type == "skill").cloned().unwrap();
        assert_eq!(skill.str_field("sha").map(str::len), Some(64), "{:?}", skill.fields);
        crate::shared::project_summary::record_delivered(root, "x").unwrap();
        assert_eq!(mustard_core::io::project_summary::read(root)[0].skills, ["falta-teste"]);
    }
}
