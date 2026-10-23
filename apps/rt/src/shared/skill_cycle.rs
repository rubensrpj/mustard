//! A vida das skills que o Mustard cria, depois do nascimento
//! ([`super::failure_skills`]), com as contas no registro do projeto
//! ([`mustard_core::io::mustard_skills`]).
//!
//! A escolha: quando sobram mais candidatas do que cabem no bloco de uma
//! tarefa, o Jev responde, de cada uma, se ela faz a coisa específica que a
//! tarefa pede, vendo a descrição inteira e o começo do corpo; entram as de
//! maior chance a partir de [`PICK_AT_LEAST`], e a nomeada pela tarefa entra
//! sempre, sem pergunta. Sem Jev, as primeiras, como sempre. A pergunta sai
//! antes das travas da rodada, e a resposta fica gravada
//! ([`crate::commands::flow::round::dispatch_jev`]); com as travas presas, a
//! rodada só a lê, e a onda nova sem decisão não sai nesta rodada, só ela.
//! Com o Jev ligado falhando, a escolha vai à reserva, e o recibo a manda na
//! rodada seguinte, com a dúvida abaixo do corte. A onda que a montagem do
//! backlog formou e a escolha segurou não volta a ser montada
//! ([`held_for_picks`]): sai com o número que ganhou.
//!
//! A conta: cada envio soma, a cada skill do Mustard, uma candidatura quando
//! ela foi candidata a alguma tarefa e uma entrega por bloco que a levou.
//!
//! A atualização: depois do commit de cada onda, a skill cujo arquivo citado
//! mudou vai ao Jev com a mudança, e o sim ou a dúvida viram a tarefa de
//! atualizá-la, com o skill-creator inteiro no bloco, como a de criar; o
//! arquivo citado que sumiu vira a tarefa sem pergunta. A onda que recebeu a
//! skill e foi reprovada pela mesma falha que a motivou também. Nenhuma skill
//! é reescrita sozinha.
//!
//! A saída: a skill candidata em [`IDLE_CANDIDACIES`] envios sem nunca ir com
//! uma tarefa, ou descartada sem ganho em dois testes seguidos, sai de
//! `.claude/skills/`, com a cópia de segurança guardada, o evento `skill`
//! `retired` e o aviso no relato da rodada.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{SpecLog, TASK_TITLE_MAX};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::domain::text::cut_line;
use mustard_core::domain::wave_prompt::{SkillOption, SkillTie};
use mustard_core::io::failures::{self, Failure, SkillTask};
use mustard_core::io::mustard_skills::{self as registry, IDLE_CANDIDACIES, MustardSkill, RETIRED};
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{Flight, WavePrompt};
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use crate::commands::flow::round::dispatch_jev::{Asker, Outcome, reserved};
use crate::shared::failure_skills::{copy_folder, runs, skill_dir, target_spec};
use crate::shared::jev::yes_no::{self, YesNo};
use crate::shared::secret::without_secrets;

/// O nome que a escolha das skills de uma tarefa grava na chamada ao Jev.
pub(crate) const PICK_CALL: &str = "skill pick";

/// A chance mínima de uma candidata entrar no bloco: abaixo dela, a skill
/// não entra, nem para completar o bloco.
pub(crate) const PICK_AT_LEAST: f64 = 0.30;

/// A pergunta da escolha das skills da tarefa `tie`: a tarefa no estado,
/// cada candidata uma entrada.
fn pick_question(tie: &SkillTie) -> YesNo<'static> {
    let mut context = Map::new();
    context.insert("task".to_string(), json!({ "title": without_secrets(&tie.title), "text": without_secrets(&tie.text) }));
    YesNo {
        context,
        section: "skills",
        question: "Does `{entry}` do the specific thing `task` asks for?",
        focus: "Read the description and the start of the body of the skill. Judge only whether the skill does the specific thing this task asks. Sharing the area or the files of the task alone is a no.",
        yes: "The skill does the specific thing the task asks for",
        no: "The skill does something else, even when it covers the same area or files",
    }
}

/// O que o Jev vê de uma candidata: o nome, a descrição inteira e o começo
/// do corpo, sem segredo.
fn shown_option(option: &SkillOption) -> Value {
    json!({ "name": option.name, "description": without_secrets(&option.description), "body": without_secrets(&option.excerpt) })
}

/// As skills que entram no bloco da tarefa `tie` pela chance de sim de cada
/// candidata (`chances`, pela chave): as de maior chance, de `cut` para
/// cima, até caber.
fn ranked(tie: &SkillTie, chances: &BTreeMap<String, f64>, cut: f64) -> Vec<String> {
    let mut ranked: Vec<(f64, &SkillOption)> = tie
        .candidates
        .iter()
        .enumerate()
        .filter_map(|(at, option)| chances.get(&candidate_key(at)).filter(|chance| **chance >= cut).map(|chance| (*chance, option)))
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    ranked.into_iter().take(tie.room).map(|(_, option)| option.path.clone()).collect()
}

/// A chave da candidata de número `at` na pergunta da tarefa.
fn candidate_key(at: usize) -> String {
    format!("s{}", at + 1)
}

/// As perguntas das tarefas `ties`, uma por tarefa, com as candidatas de
/// cada uma.
fn pick_groups(ties: &[&SkillTie]) -> (Vec<YesNo<'static>>, Vec<Vec<(String, Value)>>) {
    let questions = ties.iter().map(|tie| pick_question(tie)).collect();
    let entries =
        ties.iter().map(|tie| tie.candidates.iter().enumerate().map(|(at, option)| (candidate_key(at), shown_option(option))).collect()).collect();
    (questions, entries)
}

/// A escolha das skills das tarefas de um envio.
#[derive(Debug, Default)]
pub(crate) struct Picked {
    /// As skills escolhidas, pelo número da tarefa.
    pub(crate) picks: BTreeMap<u64, Vec<String>>,
    /// As ondas que não saem nesta rodada: a escolha de cada uma espera a
    /// decisão do Jev, ou o recibo da reserva dele.
    pub(crate) held: BTreeSet<u64>,
}

/// As perguntas da escolha das skills de uma onda: as tarefas dela com
/// empate, a pergunta de cada uma e os pedidos ao Jev.
struct WaveTies<'a> {
    wave: u64,
    ties: Vec<&'a SkillTie>,
    parts: Vec<Vec<(String, Vec<String>)>>,
    payloads: Vec<String>,
}

impl<'a> WaveTies<'a> {
    /// As perguntas da escolha das skills das tarefas com empate do pedido
    /// `prompt`.
    fn of(prompt: &'a WavePrompt) -> Self {
        let ties: Vec<&SkillTie> = prompt.skill_ties.iter().collect();
        let (questions, entries) = pick_groups(&ties);
        let parts: Vec<Vec<(String, Vec<String>)>> = questions.iter().zip(&entries).map(|(question, entries)| question.payloads(entries)).collect();
        let payloads: Vec<String> = parts.iter().flatten().map(|(payload, _)| payload.clone()).collect();
        Self { wave: prompt.wave, ties, parts, payloads }
    }

    /// As skills de cada tarefa pelas respostas `docs`, uma por pedido, com
    /// a chance mínima `cut`; `None` quando alguma não se lê.
    fn picked(&self, docs: &[Value], cut: f64) -> Option<BTreeMap<u64, Vec<String>>> {
        let mut at = 0;
        let mut out = BTreeMap::new();
        for (tie, group) in self.ties.iter().zip(&self.parts) {
            let chances = yes_no::chances(group, docs.get(at..at + group.len())?)?;
            at += group.len();
            out.insert(tie.task, ranked(tie, &chances, cut));
        }
        Some(out)
    }

    /// [`Self::picked`] só das tarefas que se leem, a escolha de sempre
    /// quando a reserva não se prepara.
    fn readable(&self, docs: Option<&[Value]>) -> BTreeMap<u64, Vec<String>> {
        let Some(docs) = docs else { return BTreeMap::new() };
        let mut at = 0;
        let mut out = BTreeMap::new();
        for (tie, group) in self.ties.iter().zip(&self.parts) {
            if let Some(chances) = docs.get(at..at + group.len()).and_then(|answered| yes_no::chances(group, answered)) {
                out.insert(tie.task, ranked(tie, &chances, PICK_AT_LEAST));
            }
            at += group.len();
        }
        out
    }
}

/// As skills escolhidas para as tarefas dos pedidos `built` que têm mais
/// candidatas do que cabem, com o Jev `jev` do projeto, o que a rodada já
/// carregou (desligado, sem ele), decididas por `asker`. Sem tarefa assim, ou
/// sem Jev, nenhuma. Antes das travas, o recibo da reserva já entregue da
/// escolha de uma onda vale primeiro, e as ondas ainda sem decisão vão ao Jev
/// numa chamada; a resposta de cada uma fica gravada, ou vai à reserva. Com as
/// travas presas, só se lê a decisão gravada. A do recibo põe a dúvida abaixo
/// do corte: só o sim certo entra. A onda de `holdable` sem decisão fica fora
/// da rodada; a outra, e a cuja reserva não se prepara, leva a escolha que se
/// lê e, no resto, as primeiras.
pub(crate) fn picks_decided(built: &[WavePrompt], holdable: &BTreeSet<u64>, jev: Option<&yes_no::AskFn<'_>>, asker: &Asker<'_>) -> Picked {
    let mut out = Picked::default();
    let Some(invoke) = jev else { return out };
    let ones: Vec<WaveTies<'_>> = built.iter().filter(|prompt| !prompt.skill_ties.is_empty()).map(WaveTies::of).collect();
    if asker.asks() {
        let asking: Vec<(&WaveTies<'_>, Vec<String>)> = ones
            .iter()
            .filter(|one| !asker.take_receipt(PICK_CALL, &one.payloads))
            .map(|one| (one, asker.missing(PICK_CALL, &one.payloads)))
            .filter(|(_, wanted)| !wanted.is_empty())
            .collect();
        let payloads: Vec<String> = asking.iter().flat_map(|(_, wanted)| wanted.iter().cloned()).collect();
        if !payloads.is_empty() {
            let started = std::time::Instant::now();
            let docs = invoke(&payloads).filter(|docs| docs.len() == payloads.len());
            let mut at = 0;
            for (_, wanted) in &asking {
                let answered = docs.as_ref().map(|docs| docs[at..at + wanted.len()].to_vec());
                at += wanted.len();
                asker.settle(PICK_CALL, wanted, answered, true, started);
            }
        }
    }
    for one in &ones {
        match asker.decided(PICK_CALL, &one.payloads) {
            Some(Outcome::Answered(docs)) => {
                let cut = if reserved(&docs) { yes_no::SURE } else { PICK_AT_LEAST };
                match one.picked(&docs, cut) {
                    Some(picked) => out.picks.extend(picked),
                    None => out.picks.extend(one.readable(Some(&docs))),
                }
            }
            Some(Outcome::Rule | Outcome::Waiting) => {}
            None if holdable.contains(&one.wave) => {
                out.held.insert(one.wave);
            }
            None => {}
        }
    }
    out
}

/// Os pedidos das ondas `outgoing` da spec `spec` do projeto `root`, lida em
/// `log`, no idioma `lang`, com as ondas fora em `flight`: montados uma vez e,
/// quando alguma tarefa tem mais skills candidatas do que cabem, montados de
/// novo com a escolha do Jev `jev`, o que a rodada já carregou, decidida por
/// `asker`, que fica em `flight`. A
/// onda nova (de `next`) cuja escolha espera a decisão sai de `next`, de
/// `outgoing` e das ondas fora, sem pedido montado, e volta à parte.
pub(crate) fn prompts_with_picks(
    root: &Path,
    spec: &str,
    (log, lang): (&SpecLog, Locale),
    flight: &mut Flight,
    (next, outgoing): (&mut Vec<u64>, &mut BTreeSet<u64>),
    jev: Option<&yes_no::AskFn<'_>>,
    asker: &Asker<'_>,
) -> (Vec<WavePrompt>, BTreeSet<u64>) {
    let built = mustard_core::io::wave_prompt::prompts(root, spec, log, lang, flight, outgoing);
    let Picked { picks, held } = picks_decided(&built, &next.iter().copied().collect(), jev, asker);
    if picks.is_empty() && held.is_empty() {
        return (built, held);
    }
    flight.skill_picks = picks;
    flight.running.retain(|wave| !held.contains(wave));
    next.retain(|wave| !held.contains(wave));
    outgoing.retain(|wave| !held.contains(wave));
    (mustard_core::io::wave_prompt::prompts(root, spec, log, lang, flight, outgoing), held)
}

/// O que a resposta da rodada diz das ondas `held`, cuja escolha das skills
/// espera a decisão: o aviso de cada uma, em `warnings`, e a frase do próximo
/// passo de cada uma, no idioma `lang`.
pub(crate) fn held_notes(held: BTreeSet<u64>, warnings: &mut Vec<Value>, lang: Locale) -> Vec<String> {
    let lines: Vec<String> = held.iter().map(|wave| translate("round.skill_pick_reserve", lang).replace("{wave}", &wave.to_string())).collect();
    warnings.extend(held.iter().zip(&lines).map(|(wave, hint)| json!({ "reason": "skill-pick-reserve", "wave": wave, "hint": hint })));
    lines
}

/// As ondas de `waves`, montadas pelo backlog e não enviadas, da spec `spec`
/// do projeto `root`, lida em `log`, que a rodada que as montou segurou à
/// espera da escolha das skills pelo Jev `jev`, o que a rodada já carregou:
/// alguma tarefa delas tem mais skills candidatas do que cabem, e algum
/// pedido dessa escolha não tinha decisão gravada, por `asker`, quando a onda
/// foi gravada. A montagem não as desfaz: cada uma sai com o número que
/// ganhou, na rodada em que a decisão chega. A onda que já tinha a decisão e
/// não saiu por outro motivo fica de fora, e, sem o Jev da escolha das
/// skills, nenhuma onda é segurada. O empate das skills de uma tarefa não
/// depende do idioma `lang` nem das ondas que estão fora: o pedido se monta
/// sem elas.
pub(crate) fn held_for_picks(
    root: &Path,
    spec: &str,
    (log, lang): (&SpecLog, Locale),
    waves: &BTreeSet<u64>,
    jev: Option<&yes_no::AskFn<'_>>,
    asker: &Asker<'_>,
) -> BTreeSet<u64> {
    if waves.is_empty() || jev.is_none() {
        return BTreeSet::new();
    }
    let formed = log.last_by_wave("wave");
    let built = mustard_core::io::wave_prompt::prompts(root, spec, log, lang, &Flight::default(), waves);
    built
        .iter()
        .filter(|prompt| !prompt.skill_ties.is_empty())
        .filter(|prompt| formed.get(&prompt.wave).is_some_and(|at| asker.undecided_at(PICK_CALL, &WaveTies::of(prompt).payloads, *at)))
        .map(|prompt| prompt.wave)
        .collect()
}

/// Conta os envios dos pedidos `sent` no registro do projeto `root`: as
/// candidatas de cada pedido e as skills de cada bloco de tarefa. Sem
/// registro, sem skill do Mustard, nada a contar. Melhor esforço: a falha
/// fica para o envio seguinte.
pub(crate) fn count_sends(root: &Path, sent: &[&WavePrompt]) {
    if sent.is_empty() || !registry::path(root).is_file() {
        return;
    }
    let _ = registry::change(root, |skills| {
        for prompt in sent {
            let delivered: Vec<Vec<String>> = prompt.tasks.iter().map(|block| block.skills.clone()).collect();
            skills.count_send(&prompt.skill_candidates, &delivered);
        }
    });
}

/// Os arquivos de `value`, uma lista de textos.
fn texts(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

/// A onda cuja cópia contém `start`, pelo envio dela em `log`: a que gravou
/// a skill de dentro da cópia.
fn writer_wave(log: &SpecLog, start: &Path) -> Option<u64> {
    let here = start.to_string_lossy();
    log.events
        .iter()
        .rev()
        .filter(|e| e.event_type == "send")
        .find(|e| e.str_field("copy").is_some_and(|copy| crate::shared::paths::below(copy, &here).is_some()))
        .and_then(|e| e.wave())
}

/// Os grupos de falha cuja tarefa de criar skill está na spec `spec`, na onda
/// `wave` (em qualquer onda, quando não se sabe qual), pelo registro das
/// falhas do projeto `root`.
fn motivating_groups(root: &Path, spec: &str, log: &SpecLog, wave: Option<u64>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for row in failures::read(root) {
        let Some(task) = row.skill_task.as_ref().filter(|task| task.spec == spec).and_then(|task| task.task) else { continue };
        let Some(current) = log.current(task) else { continue };
        let creates = current.str_field("skill") == Some(mustard_core::io::skill_creator::NAME);
        if creates && (wave.is_none() || current.wave() == wave) && !out.iter().any(|group| group == row.group_id()) {
            out.push(row.group_id().to_string());
        }
    }
    out
}

/// Depois de gravado o evento `skill` número `id` na spec `spec` de `start`:
/// o registro do projeto guarda a criação, a atualização ou o descarte, com
/// o ganho, os arquivos citados — os do evento ou, sem eles, os que o
/// `SKILL.md` cita —, a onda que a gravou e os grupos de falha que a
/// motivaram. Devolve os avisos, em `lang`.
pub(crate) fn skill_recorded(start: &Path, spec: &str, id: u64, lang: Locale) -> Vec<String> {
    let root = store::spec_root(start);
    let Some(log) = store::spec_file(&root, spec).ok().and_then(|file| store::read(&file).ok().flatten()) else {
        return Vec::new();
    };
    let Some(event) = log.get(id) else { return Vec::new() };
    let (Some(name), Some(action)) = (event.str_field("name").map(str::trim), event.str_field("action").map(str::trim)) else {
        return Vec::new();
    };
    let mut files = texts(event.fields.get("files"));
    if files.is_empty() && matches!(action, "created" | "updated") {
        let text = std::fs::read_to_string(skill_dir(&root, name).join("SKILL.md")).unwrap_or_default();
        files = mustard_core::domain::project_map::cited_paths(&text);
    }
    let wave = writer_wave(&log, start);
    let groups = if action == "created" { motivating_groups(&root, spec, &log, wave) } else { Vec::new() };
    let text = event.str_field("text").unwrap_or_default();
    match registry::change(&root, |skills| skills.record(name, action, event.at(), text, &files, (spec, wave), &groups)) {
        Ok(()) => Vec::new(),
        Err(error) => vec![translate("skills.cycle_failed", lang).replace("{detail}", &error.to_string())],
    }
}

/// Por que a skill precisa de atualização.
enum Why {
    /// O commit `sha` mudou os arquivos citados `files`, e o Jev deu
    /// `chance` de a mudança `diff` contradizer a skill.
    Change { sha: String, files: Vec<String>, chance: f64, diff: String },
    /// O commit `sha` apagou os arquivos citados `files`.
    Gone { sha: String, files: Vec<String> },
    /// A onda `wave` da spec `spec` recebeu a skill e foi reprovada pela
    /// mesma falha que a motivou, pelo motivo `reason`, nos arquivos `files`.
    Failure { spec: String, wave: u64, reason: String, files: Vec<String> },
}

impl Why {
    /// Os arquivos do motivo.
    fn files(&self) -> &[String] {
        match self {
            Self::Change { files, .. } | Self::Gone { files, .. } | Self::Failure { files, .. } => files,
        }
    }

    /// O motivo, no idioma `lang`, para a parte do agente.
    fn text(&self, lang: Locale) -> String {
        let listed = |files: &[String]| files.iter().map(|file| format!("`{file}`")).collect::<Vec<_>>().join(", ");
        match self {
            Self::Change { sha, files, chance, diff } => translate("skills.update_why_change", lang)
                .replace("{sha}", &sha.chars().take(12).collect::<String>())
                .replace("{files}", &listed(files))
                .replace("{chance}", &format!("{chance:.2}"))
                .replace("{diff}", diff),
            Self::Gone { sha, files } => {
                translate("skills.update_why_gone", lang).replace("{sha}", &sha.chars().take(12).collect::<String>()).replace("{files}", &listed(files))
            }
            Self::Failure { spec, wave, reason, .. } => translate("skills.update_why_failure", lang)
                .replace("{spec}", spec)
                .replace("{wave}", &wave.to_string())
                .replace("{reason}", &cut_line(reason, REASON_CHARS)),
        }
    }
}

/// Quantos caracteres do motivo de uma reprovação vão à tarefa.
const REASON_CHARS: usize = 600;

/// Quantos arquivos a tarefa de atualizar declara.
const TASK_FILES_MAX: usize = 8;

/// A tarefa de atualizar a skill `skill`, de nome `name`, no projeto `root`,
/// pelo motivo `why`, no idioma `lang`: o título com o nome, os arquivos pelo
/// caminho no texto, o motivo e a mudança na parte do agente, o skill-creator
/// nomeado e os arquivos citados que existem.
fn update_draft(root: &Path, name: &str, skill: &MustardSkill, why: &Why, lang: Locale) -> Map<String, Value> {
    let title = translate("skills.update_task_title", lang).replace("{name}", name);
    let title: String = title.chars().take(TASK_TITLE_MAX).collect();
    let mut shown: Vec<String> = why.files().to_vec();
    shown.extend(skill.files.iter().filter(|file| !why.files().contains(file)).cloned());
    let listed = shown.iter().take(TASK_FILES_MAX).map(|file| format!("`{file}`")).collect::<Vec<_>>().join(", ");
    let main = skill_dir(root, name).join("SKILL.md");
    let agent = translate("skills.update_task_agent", lang)
        .replace("{name}", name)
        .replace("{path}", &mustard_core::io::wave_prompt::shown(&std::path::absolute(&main).unwrap_or(main)))
        .replace("{why}", &why.text(lang));
    let files: Vec<Value> = shown.iter().filter(|file| root.join(file).exists()).take(TASK_FILES_MAX).map(|path| json!({ "path": path })).collect();
    let draft = json!({
        "title": title,
        "text": translate("skills.update_task_text", lang).replace("{name}", name).replace("{files}", &listed),
        "agent": agent,
        "skill": mustard_core::io::skill_creator::NAME,
        "files": files,
        "depends_on": [],
        "author": "binary",
    });
    draft.as_object().cloned().unwrap_or_default()
}

/// Cria, na spec aberta a partir de `spec` ([`target_spec`]), a tarefa de
/// atualizar a skill `name` do projeto de `start` pelo motivo `why`, uma por
/// skill enquanto a anterior espera numa spec que roda, e a marca no
/// registro. Devolve a tarefa gravada; nenhuma para a skill retirada, a que
/// já tem tarefa à espera ou sem spec que rode.
///
/// # Errors
///
/// A recusa da gravação ou a falha do registro, com o motivo.
fn update_task(start: &Path, spec: &str, name: &str, why: &Why, lang: Locale) -> Result<Option<Value>, String> {
    let root = store::spec_root(start);
    let Some(skill) = registry::read(&root).skills.get(name).cloned() else { return Ok(None) };
    if skill.state == RETIRED || skill.update_task.as_ref().is_some_and(|task| runs(start, &task.spec)) {
        return Ok(None);
    }
    let Some(target) = target_spec(start, spec) else { return Ok(None) };
    let draft = update_draft(&root, name, &skill, why, lang);
    let written = crate::commands::spec_events::write::record(start, &target, "task", draft, PhaseWriter::Binary).map_err(|refusal| refusal.message(lang))?;
    let id = written.written.id;
    let mark = SkillTask { spec: target.clone(), task: Some(id) };
    registry::change(&root, |skills| {
        if let Some(skill) = skills.skills.get_mut(name) {
            skill.update_task = Some(mark);
        }
    })
    .map_err(|error| error.to_string())?;
    Ok(Some(json!({ "type": "task", "spec": target, "id": id, "skill": mustard_core::io::skill_creator::NAME, "updates": name })))
}

/// O nome que a pergunta da skill desatualizada grava na chamada ao Jev.
pub(crate) const STALE_CALL: &str = "skill stale";

/// A chance a partir da qual a mudança vira tarefa: o sim, de 0,8 para cima,
/// e a dúvida, entre 0,2 e 0,8.
const STALE_AT_LEAST: f64 = 0.2;

/// Quantos caracteres do corpo da skill e da mudança o Jev vê.
const STALE_CHARS: usize = 6_000;

/// A pergunta da skill desatualizada: o corpo da skill no estado, a mudança
/// a entrada.
fn stale_question(body: &str) -> YesNo<'static> {
    let mut context = Map::new();
    context.insert("skill".to_string(), json!(without_secrets(&body.chars().take(STALE_CHARS).collect::<String>())));
    YesNo {
        context,
        section: "change",
        question: "Does the change in `{entry}` contradict what `skill` teaches?",
        focus: "Compare the removed and added lines with the instructions of the skill. A change the skill does not talk about, or that agrees with it, is a no.",
        yes: "Following the skill after this change would lead to wrong work, because the change contradicts what it teaches",
        no: "The skill still holds after the change",
    }
}

/// `true` quando o arquivo `file` é o citado `cited` ou fica na pasta dele.
fn cites(cited: &str, file: &str) -> bool {
    let cited = cited.trim_start_matches("./").trim_end_matches('/');
    !cited.is_empty() && (file == cited || file.strip_prefix(cited).is_some_and(|rest| rest.starts_with('/')))
}

/// `true` quando a hora `at` não é anterior a `since`; a que não se lê conta
/// como depois.
fn not_before(at: &str, since: &str) -> bool {
    match (chrono::DateTime::parse_from_rfc3339(at), chrono::DateTime::parse_from_rfc3339(since)) {
        (Ok(at), Ok(since)) => at >= since,
        _ => true,
    }
}

/// A mudança do commit `sha` nos arquivos `files` do projeto `root`, sem
/// segredo, cortada em [`STALE_CHARS`].
fn diff_of(root: &Path, sha: &str, files: &[String]) -> String {
    let mut args = vec!["show", "--format=", "--no-color", "--no-ext-diff", sha, "--"];
    args.extend(files.iter().map(String::as_str));
    let diff = mustard_core::platform::git::run(root, &args).out().unwrap_or_default();
    without_secrets(&diff).chars().take(STALE_CHARS).collect()
}

/// Uma skill cujo arquivo citado um commit mudou.
struct Touched {
    name: String,
    sha: String,
    files: Vec<String>,
    gone: Vec<String>,
}

/// As skills do Mustard cujos arquivos citados os commits novos da spec
/// `spec` em `log` mudaram, no projeto `root`, e o último evento de commit
/// lido. O commit da onda que criou a skill e o de antes da última criação ou
/// atualização dela não contam.
fn touched_by_commits(root: &Path, spec: &str, log: &SpecLog, skills: &registry::Registry) -> (Vec<Touched>, Option<u64>) {
    let read = skills.commits_read.get(spec).copied().unwrap_or(0);
    let commits: Vec<_> = log.visible().into_iter().filter(|e| e.event_type == "commit" && e.id > read).collect();
    let mut out: Vec<Touched> = Vec::new();
    for commit in &commits {
        let Some(sha) = commit.str_field("sha") else { continue };
        let changed = texts(commit.fields.get("files"));
        let waves = commit.ints("waves");
        for (name, skill) in skills.skills.iter().filter(|(_, skill)| skill.made && skill.state != RETIRED) {
            if skill.spec == spec && skill.wave.is_some_and(|wave| waves.contains(&wave)) || !not_before(commit.at(), &skill.since) {
                continue;
            }
            let files: Vec<String> = changed.iter().filter(|file| skill.files.iter().any(|cited| cites(cited, file))).cloned().collect();
            if files.is_empty() {
                continue;
            }
            let gone = files.iter().filter(|file| !root.join(file).exists()).cloned().collect();
            out.push(Touched { name: name.clone(), sha: sha.to_string(), files, gone });
        }
    }
    (out, commits.iter().map(|commit| commit.id).max())
}

/// A conferência das skills do Mustard depois dos commits novos da spec
/// `spec` de `start`, com o Jev `jev` (`None` quando desligado): o arquivo
/// citado que sumiu vira a tarefa de atualizar sem pergunta; a mudança vai ao
/// Jev com o corpo da skill, e o sim ou a dúvida viram a tarefa. Sem Jev, só
/// o arquivo que sumiu. O Jev ligado que não responde manda a pergunta à
/// reserva dele, e o pedido volta junto. Cada commit é lido uma vez, e só
/// conta como lido com a resposta do Jev ou do recibo da reserva, ou com o
/// Jev desligado. Devolve as tarefas gravadas e o pedido de reserva.
///
/// # Errors
///
/// A recusa da gravação ou a falha do registro, com o motivo.
fn stale_with(start: &Path, spec: &str, jev: Option<&yes_no::AskFn<'_>>, lang: Locale) -> Result<(Vec<Value>, Option<Value>), String> {
    let root = store::spec_root(start);
    let Some(log) = store::spec_file(&root, spec).ok().and_then(|file| store::read(&file).ok().flatten()) else { return Ok((Vec::new(), None)) };
    let skills = registry::read(&root);
    let (touched, last) = touched_by_commits(&root, spec, &log, &skills);
    let Some(last) = last else { return Ok((Vec::new(), None)) };
    let mut whys: Vec<(String, Why)> = Vec::new();
    let mut asked: Vec<(&Touched, String, Value)> = Vec::new();
    for change in &touched {
        if !change.gone.is_empty() {
            whys.push((change.name.clone(), Why::Gone { sha: change.sha.clone(), files: change.gone.clone() }));
            continue;
        }
        let Ok(text) = std::fs::read_to_string(skill_dir(&root, &change.name).join("SKILL.md")) else { continue };
        let body = mustard_core::domain::skill::frontmatter::body(&text);
        asked.push((change, body, json!(diff_of(&root, &change.sha, &change.files))));
    }
    let (mut read, mut reserve) = (true, None);
    if !asked.is_empty() {
        let questions: Vec<YesNo<'static>> = asked.iter().map(|(_, body, _)| stale_question(body)).collect();
        let entries: Vec<Vec<(String, Value)>> = asked.iter().map(|(_, _, diff)| vec![("c1".to_string(), diff.clone())]).collect();
        let groups: Vec<(&YesNo<'_>, &[(String, Value)])> = questions.iter().zip(&entries).map(|(question, entries)| (question, entries.as_slice())).collect();
        let answers = match yes_no::ask_all_reserved(&root, spec, STALE_CALL, &groups, jev) {
            yes_no::Decided::Chances(answers) => answers,
            yes_no::Decided::Off => Vec::new(),
            yes_no::Decided::Reserve(request) => {
                reserve = Some(request);
                Vec::new()
            }
        };
        read = reserve.is_none() && answers.iter().all(Option::is_some);
        for ((change, _, diff), answer) in asked.iter().zip(answers) {
            let Some(chance) = answer.and_then(|chances| chances.get("c1").copied()).filter(|chance| *chance >= STALE_AT_LEAST) else { continue };
            let diff = diff.as_str().unwrap_or_default().to_string();
            whys.push((change.name.clone(), Why::Change { sha: change.sha.clone(), files: change.files.clone(), chance, diff }));
        }
    }
    if read {
        registry::change(&root, |skills| {
            skills.commits_read.insert(spec.to_string(), last);
        })
        .map_err(|error| error.to_string())?;
    }
    let mut recorded = Vec::new();
    let mut done: BTreeSet<String> = BTreeSet::new();
    for (name, why) in &whys {
        if done.insert(name.clone()) {
            recorded.extend(update_task(start, spec, name, why, lang)?);
        }
    }
    Ok((recorded, reserve))
}

/// `true` quando a onda da falha `row` recebeu a skill `name` no bloco de
/// alguma tarefa, num envio anterior à reprovação, pelo arquivo da spec dela
/// no projeto `root`.
fn received(root: &Path, row: &Failure, name: &str) -> bool {
    let Some(log) = store::spec_file(root, &row.spec).ok().and_then(|file| store::read(&file).ok().flatten()) else { return false };
    let before = row.id.rsplit_once(':').and_then(|(_, id)| id.parse::<u64>().ok()).unwrap_or(u64::MAX);
    log.events.iter().filter(|e| e.event_type == "send" && e.wave() == Some(row.wave) && e.id < before).any(|send| {
        send.fields.get("tasks").and_then(Value::as_array).into_iter().flatten().any(|block| texts(block.get("skills")).iter().any(|had| had == name))
    })
}

/// Depois de as falhas `grouped` do projeto de `start` ganharem grupo: a
/// falha de uma onda que recebeu uma skill do Mustard e caiu no mesmo grupo
/// que a motivou vira a tarefa de atualizá-la. Devolve as tarefas gravadas.
///
/// # Errors
///
/// A recusa da gravação ou a falha do registro, com o motivo.
pub(crate) fn after_failures(start: &Path, grouped: &[String], lang: Locale) -> Result<Vec<Value>, String> {
    let root = store::spec_root(start);
    if grouped.is_empty() || !registry::path(&root).is_file() {
        return Ok(Vec::new());
    }
    let skills = registry::read(&root);
    let rows = failures::read(&root);
    let mut recorded = Vec::new();
    for row in rows.iter().filter(|row| grouped.contains(&row.id)) {
        for (name, _) in skills.skills.iter().filter(|(_, skill)| skill.made && skill.state != RETIRED && skill.groups.iter().any(|group| group == row.group_id())) {
            if received(&root, row, name) {
                let why = Why::Failure { spec: row.spec.clone(), wave: row.wave, reason: row.reason.clone(), files: row.files.clone() };
                recorded.extend(update_task(start, &row.spec, name, &why, lang)?);
            }
        }
    }
    Ok(recorded)
}

/// O motivo da saída `reason` ([`MustardSkill::retire_reason`]), no idioma
/// `lang`.
fn retire_text(reason: &str, lang: Locale) -> String {
    let count = if reason == "idle" { IDLE_CANDIDACIES } else { registry::DISCARDS_IN_ROW };
    translate(&format!("skills.retired_reason.{reason}"), lang).replace("{count}", &count.to_string())
}

/// Tira de `.claude/skills/` cada skill do Mustard do projeto de `start` que
/// deve sair ([`MustardSkill::retire_reason`]): a cópia de segurança fica,
/// com a pasta copiada antes quando ainda faltava, o evento `skill` `retired`
/// entra na spec `spec` e o registro a marca retirada. Devolve os eventos
/// gravados e os avisos da rodada, cada um com o motivo.
fn retire(start: &Path, spec: &str, lang: Locale) -> (Vec<Value>, Vec<(&'static str, String)>) {
    let root = store::spec_root(start);
    let due: Vec<(String, &'static str)> =
        registry::read(&root).skills.iter().filter_map(|(name, skill)| skill.retire_reason().map(|reason| (name.clone(), reason))).collect();
    let (mut recorded, mut warnings) = (Vec::new(), Vec::new());
    for (name, reason) in due {
        let why = retire_text(reason, lang);
        let draft = json!({ "name": name, "action": "retired", "text": why, "author": "binary" });
        let written = match crate::commands::spec_events::write::record(start, spec, "skill", draft.as_object().cloned().unwrap_or_default(), PhaseWriter::Binary) {
            Ok(written) => written,
            Err(refusal) => {
                warnings.push(("skill-cycle-failed", translate("skills.cycle_failed", lang).replace("{detail}", &refusal.message(lang))));
                continue;
            }
        };
        let (main, backup): (PathBuf, PathBuf) = (skill_dir(&root, &name), mustard_core::io::wave_prompt::skills_backup_dir(&root).join(&name));
        if main.join("SKILL.md").is_file() && !backup.join("SKILL.md").is_file() {
            let _ = copy_folder(&main, &backup);
        }
        let _ = std::fs::remove_dir_all(&main);
        let at = mustard_core::platform::time::now_iso8601();
        if let Err(error) = registry::change(&root, |skills| skills.record(&name, "retired", &at, &why, &[], (spec, None), &[])) {
            warnings.push(("skill-cycle-failed", translate("skills.cycle_failed", lang).replace("{detail}", &error.to_string())));
        }
        recorded.push(json!({ "type": "skill", "spec": spec, "id": written.written.id, "name": name, "action": "retired" }));
        let hint = translate("skills.retired", lang)
            .replace("{name}", &name)
            .replace("{reason}", &why)
            .replace("{backup}", &mustard_core::io::wave_prompt::shown(&backup));
        warnings.push(("skill-retired", hint));
    }
    (recorded, warnings)
}

/// Depois da rodada da spec `spec` de `start`, com as travas dela soltas: a
/// conferência das skills do Mustard contra os commits novos, com o Jev do
/// projeto, e a saída das que não servem. Sem skill do Mustard, nada. Devolve
/// as tarefas e os eventos gravados, os avisos, cada um com o motivo, em
/// `lang`, e o pedido de reserva da pergunta que o Jev ligado não respondeu.
pub(crate) fn after_round(start: &Path, spec: &str, lang: Locale) -> (Vec<Value>, Vec<(&'static str, String)>, Option<Value>) {
    let root = store::spec_root(start);
    if !registry::path(&root).is_file() {
        return (Vec::new(), Vec::new(), None);
    }
    let jev = yes_no::project_jev(&root, spec, STALE_CALL);
    let (mut recorded, mut warnings, reserve) = match stale_with(start, spec, jev.as_deref(), lang) {
        Ok((tasks, reserve)) => (tasks, Vec::new(), reserve),
        Err(detail) => (Vec::new(), vec![("skill-cycle-failed", translate("skills.cycle_failed", lang).replace("{detail}", &detail))], None),
    };
    let (retired, said) = retire(start, spec, lang);
    recorded.extend(retired);
    warnings.extend(said);
    (recorded, warnings, reserve)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mustard_core::io::wave_prompt::prompts;
    use mustard_core::platform::i18n::Locale;

    use super::*;
    use crate::commands::flow::round::dispatch_jev::Phase;

    fn write_skill(root: &Path, name: &str, scoped: bool) {
        let dir = root.join("apps/rt/.claude/skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let paths = if scoped { "paths:\n  - apps/rt/src/**\n" } else { "" };
        std::fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\ndescription: Does {name}.\n{paths}---\n\nBody of {name}.\n")).unwrap();
    }

    /// A project with five skills scoped to `apps/rt/src` and one named
    /// skill, and a wave with one task on `apps/rt/src/a.rs` that names the
    /// named skill when `named`.
    fn project(root: &Path, named: bool) -> mustard_core::domain::spec_events::SpecLog {
        std::fs::create_dir_all(root.join("apps/rt/src")).unwrap();
        std::fs::write(root.join("apps/rt/src/a.rs"), "fn um() {}\n").unwrap();
        for name in ["a1", "a2", "a3", "a4", "a5"] {
            write_skill(root, name, true);
        }
        write_skill(root, "somar", false);
        let mut task = json!({"wave":1,"title":"Somar o total","text":"Somar em centavos.","files":[{"path":"apps/rt/src/a.rs"}]});
        if named {
            task["skill"] = json!("somar");
        }
        let lines = [json!({"type":"wave","n":1,"text":"A onda","criteria":[],"done_when":"passa"}), {
            let mut task = task;
            task["type"] = json!("task");
            task
        }];
        let text: String = lines
            .iter()
            .enumerate()
            .map(|(at, line)| {
                let mut line = line.clone();
                (line["v"], line["id"], line["at"]) = (json!(1), json!(at + 1), json!("2026-10-20T10:00:00Z"));
                line.to_string() + "\n"
            })
            .collect();
        mustard_core::domain::spec_events::parse_log(&text)
    }

    /// The Jev answering each candidate by its name, from `chances`.
    fn jev(chances: &'static [(&'static str, f64)]) -> impl Fn(&[String]) -> Option<Vec<Value>> {
        move |payloads| {
            let docs = payloads.iter().map(|payload| {
                let body: Value = serde_json::from_str(payload).unwrap();
                let answers: Map<String, Value> = body["questions"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(|key| {
                        let name = body["state"]["skills"][key]["name"].as_str().unwrap().to_string();
                        let chance = chances.iter().find(|(had, _)| *had == name).map_or(0.0, |(_, chance)| *chance);
                        (key.clone(), json!({"type":"noul","noul":chance}))
                    })
                    .collect();
                json!({ "answers": answers })
            });
            Some(docs.collect())
        }
    }

    /// The skills that reach the block of the only task, after the choice
    /// that `invoke` makes (none without the Jev).
    fn chosen(root: &Path, named: bool, invoke: Option<&yes_no::AskFn<'_>>) -> Vec<String> {
        let log = project(root, named);
        let waves = BTreeSet::from([1]);
        let mut flight = Flight::default();
        let built = prompts(root, "x", &log, Locale::PtBr, &flight, &waves);
        let ties: Vec<&SkillTie> = built.iter().flat_map(|prompt| &prompt.skill_ties).collect();
        if let Some(invoke) = invoke {
            assert_eq!(ties.len(), 1, "five candidates for a block that takes three are one tie");
            let asker = Asker::new(root, root, "x", &log, Phase::Ask);
            flight.skill_picks = picks_decided(&built, &BTreeSet::new(), Some(invoke), &asker).picks;
        }
        prompts(root, "x", &log, Locale::PtBr, &flight, &waves).remove(0).tasks[0].skills.clone()
    }

    const CHANCES: &[(&str, f64)] = &[("a1", 0.1), ("a2", 0.9), ("a3", 0.5), ("a4", 0.35), ("a5", 0.2), ("somar", 0.0)];

    #[test]
    fn with_five_candidates_only_the_three_with_a_chance_of_at_least_030_enter_and_the_named_one_always() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(chosen(dir.path(), false, Some(&jev(CHANCES))), ["a2", "a3", "a4"]);
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(chosen(dir.path(), true, Some(&jev(CHANCES))), ["somar", "a2", "a3"], "the named skill enters without a question");
    }

    #[test]
    fn with_every_candidate_below_030_no_candidate_enters() {
        let low: &'static [(&'static str, f64)] = &[("a1", 0.29), ("a2", 0.1), ("a3", 0.0), ("a4", 0.2), ("a5", 0.05)];
        let dir = tempfile::tempdir().unwrap();
        assert!(chosen(dir.path(), false, Some(&jev(low))).is_empty());
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(chosen(dir.path(), true, Some(&jev(low))), ["somar"]);
    }

    #[test]
    fn without_the_jev_the_first_three_enter() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let log = project(root, false);
        let built = prompts(root, "x", &log, Locale::PtBr, &Flight::default(), &BTreeSet::from([1]));
        let picked = picks_decided(&built, &BTreeSet::from([1]), None, &Asker::new(root, root, "x", &log, Phase::Ask));
        assert!(picked.picks.is_empty() && picked.held.is_empty(), "no Jev in the tests, no choice and no wait");
        assert_eq!(built[0].tasks[0].skills, ["a1", "a2", "a3"]);
        assert_eq!(built[0].skill_candidates, ["a1", "a2", "a3", "a4", "a5"]);
    }

    /// Each question sees the task by its title and text, and the candidate
    /// by its whole description and the start of its body.
    #[test]
    fn each_question_sees_the_task_and_the_description_and_body_start_of_the_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let log = project(root, false);
        let built = prompts(root, "x", &log, Locale::PtBr, &Flight::default(), &BTreeSet::from([1]));
        let asked = std::cell::RefCell::new(Vec::<Value>::new());
        let answer = jev(CHANCES);
        let invoke = |payloads: &[String]| {
            asked.borrow_mut().extend(payloads.iter().map(|payload| serde_json::from_str::<Value>(payload).unwrap()));
            answer(payloads)
        };
        picks_decided(&built, &BTreeSet::new(), Some(&invoke), &Asker::new(root, root, "x", &log, Phase::Ask));
        let asked = asked.borrow();
        assert_eq!(asked[0]["state"]["task"], json!({"title":"Somar o total","text":"Somar em centavos."}));
        assert_eq!(asked[0]["state"]["skills"]["s2"], json!({"name":"a2","description":"Does a2.","body":"Body of a2."}));
        assert_eq!(asked[0]["questions"]["s2"]["instructions"]["question"], "Does `skills.s2` do the specific thing `task` asks for?");
        assert_eq!(asked[0]["questions"].as_object().unwrap().len(), 5, "one question per candidate");
    }
}
