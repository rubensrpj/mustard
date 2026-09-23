//! `mustard-rt run plan [--spec <nome>]` — o passo do plano: monta o pedido
//! de cada onda, confere tudo e leva a spec do levantamento para o plano.
//!
//! É a porta entre o levantamento e a aprovação. Depois que a especificação,
//! as ondas e as tarefas estão gravadas, este comando monta o pedido de cada
//! onda a partir dos eventos — com as lições e as skills —, confere o plano
//! contra o código real, refaz o índice, grava a fase `plan` pela mesma porta
//! de gravação de fase das outras, prepara a cópia da spec para o banco de
//! dados da página e responde o próximo passo: publique a página que ainda
//! não foi publicada, copie os lotes para o banco e faça a pergunta de
//! aprovação. O item que ainda guarda um trecho com cara de segredo sai dito,
//! para ser expurgado; ele fica fora da cópia.
//!
//! **O que trava** e segura a pergunta até ser corrigido: ponto do
//! levantamento aberto; erro de montagem do plano (ciclo entre ondas, e
//! tarefa ou dependência apontando uma onda que não existe); skill que a
//! conferência recusa; arquivo citado que não existe e não está marcado
//! como novo; tarefa que mexe em código sem dizer em que arquivo, que volta
//! com os arquivos que o mapa sugere; e tarefa cujo texto não casa com onda
//! nenhuma do plano.
//!
//! O item combinado sem dono — nenhuma tarefa de uma onda do plano o cobre,
//! ele não diz as ondas dele nem vale no projeto todo — não trava nem avisa:
//! a análise antes do envio de cada onda decide se ele vai para ela.
//!
//! **O que só avisa**, e a decisão fica com quem aprova: arquivo citado fora
//! do git (um agente noutra sessão ou máquina não o vê); nome citado que o
//! mapa não acha; ondas que saem na mesma rodada e dividem arquivo; onda com
//! partes independentes, que deve sair dividida em ondas paralelas; spec com
//! partes independentes, pela mesma conta, que pode ser dividida; item
//! combinado de uma onda que nenhuma tarefa cobre — menos o marcado como "não
//! vira código", que traz o motivo na linha dele, e o do projeto, que vale
//! sempre; contrato que nenhum critério cita; tarefa que podia nomear uma
//! skill; tarefa cujo texto não casa com o texto da onda dela, que volta
//! dizendo com qual onda ele casaria melhor; e o comando de compilar ou de
//! testar que o `mustard.json` ainda não declara, com o campo a preencher.
//!
//! A tarefa não tem nota de trabalho: a nota do Scrum saiu inteira, campo,
//! recusas e soma, e nenhum texto impresso por este passo cita a escala.
//!
//! As conferências das tarefas olham só as ondas que ainda vêm: a tarefa de
//! onda que já tem registro de entrega não é conferida, porque o que ela fez
//! está provado pelo código que entrou, pelo commit que a carrega e pela
//! revisão que a aprovou, e não pelo texto que a descreveu.
//!
//! Cada achado, dos que travam e dos que só avisam, é gravado como anotação
//! no arquivo de eventos quando este comando roda, com o rótulo do achado do
//! plano, e a página o mostra de lá, na seção própria que vem antes das
//! anotações: as duas conferências que olham para fora do arquivo — o arquivo
//! citado existe, o arquivo está no git — rodam aqui, uma vez por plano, e
//! nunca ao desenhar a página. Rodar o comando de novo não repete a anotação
//! que já está no arquivo.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::citation::{self, CitationWorld, Finding};
use mustard_core::domain::project_map::MapRefusal;
use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState, State};
use mustard_core::domain::survey::{open_points, open_refusal};
use mustard_core::domain::wave_prompt::{self, Owner};
use mustard_core::io::citation::DiskWorld;
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{prompts, Flight, WavePrompt};
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use crate::commands::flow::skill_search::{best_skill, skills_on_disk, MAP_SUGGESTIONS};
use crate::commands::spec_events::{self, read::checkout, write::record};
use crate::commands::wave::wave_overlap_check::wave_graph;
use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// As opções de `mustard-rt run plan`.
pub struct PlanOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec cujo plano é conferido; sem ela, a spec atual.
    pub spec: Option<String>,
}

/// O que a conferência do plano achou. Cada achado sabe se trava a pergunta
/// de aprovação ou se só avisa.
enum PlanFinding {
    /// Uma recusa do arquivo de eventos: ponto aberto, pedido grande demais,
    /// arquivo citado que não existe.
    Refused(Refusal),
    /// Uma skill que a conferência recusou.
    Skill { name: String, refusal: MapRefusal },
    /// Ondas que saem na mesma rodada e declaram o mesmo arquivo.
    SharedFile { waves: String, files: String, chain: String },
    /// Uma onda com partes independentes, que deve sair dividida em ondas
    /// paralelas, uma por parte.
    WaveShouldSplit { wave: u64, parts: String },
    /// Uma spec com partes independentes, que pode ser dividida, uma spec
    /// por parte.
    SpecShouldSplit { parts: String },
    /// Um ciclo de dependência entre ondas.
    WaveLoop { waves: String },
    /// Uma onda que depende de outra que o plano não tem.
    DependsOnMissing { wave: u64, on: u64 },
    /// Uma tarefa que aponta uma onda que o plano não tem.
    TaskWithoutWave { task: String, wave: u64 },
    /// Um arquivo que a tarefa cita e que o git não guarda.
    FileOutsideGit { task: String, path: String },
    /// Um nome citado que o mapa do projeto não confirma.
    Cited { task: String, finding: Finding },
    /// Um item combinado que nenhuma tarefa cobre.
    ItemWithoutTask { code: String },
    /// Um contrato que nenhum critério cita.
    ContractWithoutCriterion { code: String },
    /// Uma tarefa que mexe em código e não diz em que arquivo mexe, com os
    /// arquivos que o mapa sugere para ela.
    TaskWithoutFile { task: String, files: String },
    /// Uma tarefa cujo texto não casa com a onda dela, com a onda com que ele
    /// casa mais forte.
    TaskInTheWrongWave { task: String, wave: u64, best: u64 },
    /// Uma tarefa cujo texto não casa com onda nenhuma do plano.
    TaskMatchesNoWave { task: String, wave: u64 },
    /// Uma tarefa sem skill para a qual já existe uma skill que serve.
    TaskCouldNameASkill { task: String, skill: String },
    /// Uma tarefa sem skill cujo trabalho se repete no projeto: o plano
    /// precisa da tarefa que faz a skill dela nascer.
    SkillToBeBorn { task: String },
    /// O comando de compilar ou de testar que o projeto ainda não declarou
    /// no `mustard.json`, pelo nome do campo (`buildCommand`/`testCommand`).
    CommandNotDeclared { field: &'static str },
}

impl PlanFinding {
    /// `true` para o achado que segura a pergunta de aprovação.
    fn blocks(&self) -> bool {
        match self {
            Self::Refused(_)
            | Self::Skill { .. }
            | Self::WaveLoop { .. }
            | Self::DependsOnMissing { .. }
            | Self::TaskWithoutWave { .. }
            | Self::TaskWithoutFile { .. }
            | Self::TaskMatchesNoWave { .. } => true,
            Self::Cited { finding, .. } => finding.is_refusal(),
            Self::SharedFile { .. }
            | Self::WaveShouldSplit { .. }
            | Self::SpecShouldSplit { .. }
            | Self::FileOutsideGit { .. }
            | Self::ItemWithoutTask { .. }
            | Self::ContractWithoutCriterion { .. }
            | Self::TaskInTheWrongWave { .. }
            | Self::TaskCouldNameASkill { .. }
            | Self::SkillToBeBorn { .. }
            | Self::CommandNotDeclared { .. } => false,
        }
    }

    /// A razão curta, estável, para quem lê a saída por máquina.
    fn reason(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.reason().to_string(),
            Self::Skill { refusal, .. } => refusal.reason().to_string(),
            Self::SharedFile { .. } => "waves-share-a-file".into(),
            Self::WaveShouldSplit { .. } => "wave-should-split".into(),
            Self::SpecShouldSplit { .. } => "spec-should-split".into(),
            Self::WaveLoop { .. } => "waves-loop".into(),
            Self::DependsOnMissing { .. } => "depends-on-missing-wave".into(),
            Self::TaskWithoutWave { .. } => "task-without-wave".into(),
            Self::FileOutsideGit { .. } => "file-outside-git".into(),
            Self::Cited { finding, .. } => match finding {
                Finding::MissingFile { .. } => "cited-file-missing".into(),
                Finding::MissingLine { .. } => "cited-line-missing".into(),
                Finding::NameElsewhere { .. } => "name-elsewhere".into(),
                Finding::NameUnknown { .. } => "name-unknown".into(),
                Finding::NoMap => "names-unchecked".into(),
            },
            Self::ItemWithoutTask { .. } => "item-without-task".into(),
            Self::ContractWithoutCriterion { .. } => "contract-without-criterion".into(),
            Self::TaskWithoutFile { .. } => "task-without-file".into(),
            Self::TaskInTheWrongWave { .. } => "task-in-the-wrong-wave".into(),
            Self::TaskMatchesNoWave { .. } => "task-matches-no-wave".into(),
            Self::TaskCouldNameASkill { .. } => "task-could-name-a-skill".into(),
            Self::SkillToBeBorn { .. } => "skill-to-be-born".into(),
            Self::CommandNotDeclared { .. } => "command-not-declared".into(),
        }
    }

    /// A mensagem exata, no idioma pedido.
    fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::Refused(refusal) => refusal.message(lang),
            Self::Skill { name, refusal } => {
                format!("{name}: {}", refusal.message(lang))
            }
            Self::SharedFile { waves, files, chain } => fill(
                "plan.shared_file",
                &[("{waves}", waves.clone()), ("{files}", files.clone()), ("{chain}", chain.clone())],
            ),
            Self::WaveShouldSplit { wave, parts } => fill(
                "plan.wave_should_split",
                &[("{wave}", wave.to_string()), ("{parts}", parts.clone())],
            ),
            Self::SpecShouldSplit { parts } => fill("plan.spec_should_split", &[("{parts}", parts.clone())]),
            Self::WaveLoop { waves } => fill("plan.wave_loop", &[("{waves}", waves.clone())]),
            Self::DependsOnMissing { wave, on } => {
                fill("plan.depends_on_missing", &[("{wave}", wave.to_string()), ("{on}", on.to_string())])
            }
            Self::TaskWithoutWave { task, wave } => {
                fill("plan.task_without_wave", &[("{task}", task.clone()), ("{wave}", wave.to_string())])
            }
            Self::FileOutsideGit { task, path } => {
                fill("plan.file_outside_git", &[("{task}", task.clone()), ("{path}", path.clone())])
            }
            // A conferência das citações é a mesma do ponto do levantamento, e
            // as mensagens dela também: o número do fato vira o código da
            // tarefa que citou.
            Self::Cited { task, finding } => {
                let text = finding
                    .refusal(0)
                    .map(|refusal| refusal.message(lang))
                    .or_else(|| finding.warning(0, lang))
                    .unwrap_or_default();
                format!("{task}: {text}")
            }
            Self::ItemWithoutTask { code } => fill("plan.item_without_task", &[("{code}", code.clone())]),
            Self::ContractWithoutCriterion { code } => {
                fill("plan.contract_without_criterion", &[("{code}", code.clone())])
            }
            Self::TaskWithoutFile { task, files } => {
                let suggested = if files.is_empty() {
                    translate("plan.no_suggestion", lang).to_string()
                } else {
                    files.clone()
                };
                fill("plan.task_without_file", &[("{task}", task.clone()), ("{files}", suggested)])
            }
            Self::TaskInTheWrongWave { task, wave, best } => fill(
                "plan.task_wrong_wave",
                &[("{task}", task.clone()), ("{wave}", wave.to_string()), ("{best}", best.to_string())],
            ),
            Self::TaskMatchesNoWave { task, wave } => {
                fill("plan.task_matches_no_wave", &[("{task}", task.clone()), ("{wave}", wave.to_string())])
            }
            Self::TaskCouldNameASkill { task, skill } => {
                fill("plan.task_could_name_a_skill", &[("{task}", task.clone()), ("{skill}", skill.clone())])
            }
            Self::SkillToBeBorn { task } => fill("plan.skill_to_be_born", &[("{task}", task.clone())]),
            Self::CommandNotDeclared { field } => {
                fill("plan.command_not_declared", &[("{field}", (*field).to_string())])
            }
        }
    }

    fn to_value(&self, lang: Locale) -> Value {
        json!({ "reason": self.reason(), "hint": self.message(lang) })
    }
}

/// O núcleo testável de [`run`]. A sessão vem do ambiente. Nunca entra em
/// pânico.
pub(crate) fn plan_at(opts: &PlanOpts) -> Value {
    plan_for(opts, session_from_env().as_deref())
}

/// [`plan_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn plan_for(opts: &PlanOpts, session: Option<&str>) -> Value {
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
        Ok(None) => return refuse(&Refusal::NoSpecFile { spec }),
        Err(refusal) => return refuse(&refusal),
    };

    let running = crate::commands::flow::round::waves_in_progress(&log).into_keys().collect();
    let built = prompts(&project.root, &spec, &log, lang, &Flight { running, ..Flight::default() });
    let findings = check(&opts.root, &project.root, &spec, &log, &built);
    // Cada achado vira anotação no arquivo de eventos, aqui, uma vez por
    // plano: a página o mostra de lá, sem olhar o disco nem o git ao ser
    // desenhada. As duas conferências que olham para fora do arquivo — o
    // arquivo citado existe, o arquivo está no git — ficam nesta chamada.
    if let Err(refusal) = note_findings(&opts.root, &spec, &log, &findings, lang) {
        return refuse(&refusal);
    }
    let blocking: Vec<Value> =
        findings.iter().filter(|f| f.blocks()).map(|f| f.to_value(lang)).collect();
    let warnings: Vec<Value> =
        findings.iter().filter(|f| !f.blocks()).map(|f| f.to_value(lang)).collect();
    let waves: Vec<Value> = built
        .iter()
        .map(|p| json!({ "wave": p.wave, "lines": p.lines, "skills": p.stale_skills }))
        .collect();

    if !blocking.is_empty() {
        // O plano travado não é marco: nada vai para o banco da página, e o
        // item que guarda um trecho com cara de segredo sai dito, para ser
        // expurgado antes da cópia.
        let mut report = json!({
            "ok": false, "spec": spec, "reason": "plan-not-ready",
            "hint": translate("plan.not_ready", lang).replace("{count}", &blocking.len().to_string()),
            "waves": waves, "blocking": blocking, "warnings": warnings,
        });
        let withheld = spec_events::pages::copy::withheld(&log);
        spec_events::pages::note_withheld(&mut report, &spec, &withheld, lang);
        return report;
    }

    // A fase passa pela mesma porta de gravação de fase das outras, e ela já
    // refaz a linha da spec no índice. Uma spec que já está em plano não grava
    // nada e tem a linha refeita aqui.
    let from = State::from_log(&log).phase.unwrap_or("-").to_string();
    let recorded = if from == "plan" {
        if let Ok(paths) = mustard_core::ClaudePaths::for_project(&project.root)
            && let Err(refusal) =
                mustard_core::io::spec_index::refresh_line(&paths.spec_index_path(), &spec, &log)
        {
            return refuse(&refusal);
        }
        None
    } else {
        let mut draft = Map::new();
        draft.insert("phase".to_string(), json!("plan"));
        draft.insert("author".to_string(), json!("binary"));
        match record(&opts.root, &spec, "state", draft, PhaseWriter::Binary) {
            Ok(recorded) => Some(recorded.written.id),
            Err(refusal) => return refuse(&refusal),
        }
    };

    // O passo termina preparando a cópia para o banco da página, que é por
    // onde a spec é aprovada. A cópia acontece só nos marcos, e a aprovação é
    // um deles: a resposta manda publicar a página que ainda não tem endereço
    // e copiar os lotes, e nenhum endereço vai para a resposta ao usuário — o
    // link mora na barra de status. O item que ainda guarda um trecho com
    // cara de segredo sai dito, para ser expurgado, sem segurar a cópia nem a
    // pergunta.
    let prepared = match spec_events::pages::copy::prepare_milestone(&project.root, &spec, lang) {
        Ok(prepared) => prepared,
        Err(refusal) => return refuse(&refusal),
    };
    let mut report = json!({
        "ok": true, "spec": spec, "phase": "plan", "from": from,
        "waves": waves, "warnings": warnings,
    });
    if let Some(id) = recorded {
        report["id"] = json!(id);
    }
    // A pergunta vai com o texto exato do catálogo: a testemunha da aprovação
    // só reconhece essa pergunta, e outro texto não aprova nada.
    let ask = translate("plan.next", lang)
        .replace("{question}", translate("approval.question", lang))
        .replace("{option}", translate("approval.option", lang));
    spec_events::pages::end_milestone(&mut report, Ok(&prepared), &spec, "approval", &ask, lang);
    report
}

/// Grava cada achado da conferência como anotação, no idioma do projeto, que
/// é o da página. Cada uma leva o rótulo do achado do plano, que é por onde a
/// página as recolhe na seção própria delas. O achado que já está anotado não
/// é gravado de novo: rodar o `plan` outra vez não repete a anotação.
///
/// # Errors
///
/// A recusa da gravação da anotação.
fn note_findings(
    start: &Path,
    spec: &str,
    log: &SpecLog,
    findings: &[PlanFinding],
    lang: Locale,
) -> Result<(), Refusal> {
    let mut noted: BTreeSet<String> = log
        .block(BlockQuery::Block(Block::Notes))
        .into_iter()
        .filter(|event| event.event_type == "note")
        .filter_map(|event| event.str_field("text").map(str::to_string))
        .collect();
    for finding in findings {
        let text = finding.message(lang);
        if !noted.insert(text.clone()) {
            continue;
        }
        let mut draft = Map::new();
        draft.insert("text".to_string(), json!(text));
        draft.insert("keys".to_string(), json!([finding.reason()]));
        draft.insert("label".to_string(), json!(translate("plan.finding.label", lang)));
        draft.insert("author".to_string(), json!("binary"));
        record(start, spec, "note", draft, PhaseWriter::Binary)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// As conferências
// ---------------------------------------------------------------------------

/// Tudo que a conferência do plano acha, na ordem em que é conferido.
fn check(
    start: &Path,
    root: &Path,
    spec: &str,
    log: &SpecLog,
    built: &[WavePrompt],
) -> Vec<PlanFinding> {
    let mut out: Vec<PlanFinding> = Vec::new();
    let codes = log.codes();
    let code_of = |event: &SpecEvent| codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());

    // Nenhum ponto do levantamento aberto.
    let open = open_points(log);
    if !open.is_empty() {
        out.push(PlanFinding::Refused(open_refusal(spec, log, &open)));
    }

    // Nenhum erro de montagem do plano, e ondas em paralelo sem arquivo em
    // comum: as ondas se montam por grafo.
    let graph = wave_graph(log);
    if !graph.cycle.is_empty() {
        out.push(PlanFinding::WaveLoop { waves: join(graph.cycle.iter().map(u64::to_string)) });
    }
    for (wave, missing) in &graph.missing_depends {
        for on in missing {
            out.push(PlanFinding::DependsOnMissing { wave: *wave, on: *on });
        }
    }
    for (task, wave) in &graph.missing_task_waves {
        out.push(PlanFinding::TaskWithoutWave { task: task.clone(), wave: *wave });
    }
    for collision in &graph.collisions {
        out.push(PlanFinding::SharedFile {
            waves: join(collision.waves.iter().map(u32::to_string)),
            files: collision.files.join(", "),
            chain: collision.chain.clone(),
        });
    }

    // A onda cujas tarefas não dividem arquivo entre si tem partes
    // independentes, e sai dividida: uma onda por parte, para rodarem em
    // paralelo. É aviso, e não recusa — nem toda divisão compensa, e a
    // decisão fica com quem aprova. A onda já entregue não é mais divisível.
    let delivered = log.delivered_waves();
    let listed = |parts: &[Vec<String>]| parts.iter().map(|part| part.join(" + ")).collect::<Vec<_>>().join("; ");
    for (wave, parts) in &graph.parts {
        if delivered.contains(wave) {
            continue;
        }
        out.push(PlanFinding::WaveShouldSplit { wave: *wave, parts: listed(parts) });
    }
    // A mesma conta, olhando a spec inteira: as tarefas de todas as ondas que
    // ainda vêm entram juntas, e a spec cujas partes não dividem arquivo entre
    // si pode ser dividida, uma spec por parte. Também só avisa.
    let spec_parts = graph.spec_parts(&delivered);
    if spec_parts.len() > 1 {
        out.push(PlanFinding::SpecShouldSplit { parts: listed(&spec_parts) });
    }
    // O comando de compilar e o de testar, do mesmo `mustard.json` que
    // `prompts` já leu para montar `built`: o campo ainda não declarado, ou
    // só com o provisório do `init`, sai como aviso, com o campo a
    // preencher. Nunca recusa — a onda ainda sai, só sem a linha do comando.
    let commands = mustard_core::ProjectConfig::load(root).commands();
    if commands.build.is_none() {
        out.push(PlanFinding::CommandNotDeclared { field: "buildCommand" });
    }
    if commands.test.is_none() {
        out.push(PlanFinding::CommandNotDeclared { field: "testCommand" });
    }

    // Cada skill nomeada passa. O pedido não tem mais teto de linhas.
    for prompt in built {
        for (name, refusal) in &prompt.bad_skills {
            out.push(PlanFinding::Skill { name: name.clone(), refusal: refusal.clone() });
        }
    }

    // As citações de cada tarefa, pela mesma conferência que o ponto do
    // levantamento usa: o arquivo citado existe ou está marcado como novo, e
    // os nomes citados existem no mapa.
    let roots = store::citation_roots(start, root);
    let world = DiskWorld::new(roots, Some(root));
    let tasks: Vec<&SpecEvent> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "task")
        .collect();
    // A conferência olha só o que ainda vem: a tarefa de onda que já tem
    // registro de entrega está provada pelo código que entrou, pelo commit
    // que a carrega e pela revisão que a aprovou, e conferir de novo o texto
    // que a descreveu só acumula trava que ninguém vai consertar.
    let ahead: Vec<&SpecEvent> =
        tasks.iter().copied().filter(|task| !task.wave().is_some_and(|n| delivered.contains(&n))).collect();

    let mut cited: Vec<String> = Vec::new();
    for task in &ahead {
        let code = code_of(task);
        let files = declared_files(task);
        let text = task.str_field("text").unwrap_or_default();
        if files.is_empty() && !says_it_touches_no_file(text) {
            out.push(PlanFinding::TaskWithoutFile {
                task: code.clone(),
                files: crate::commands::map::suggested_files(root, text, MAP_SUGGESTIONS).join(", "),
            });
        }
        for (path, new) in &files {
            if !new && world.file_lines(path).is_none() {
                out.push(PlanFinding::Cited {
                    task: code.clone(),
                    finding: Finding::MissingFile { path: path.clone() },
                });
            }
            if !cited.contains(path) {
                cited.push(path.clone());
            }
        }
        for finding in citation::check(&world, "", task.str_field("text").unwrap_or_default()) {
            out.push(PlanFinding::Cited { task: code.clone(), finding });
        }
    }

    // Todo arquivo citado está no git: um agente noutra sessão ou noutra
    // máquina não vê o que só existe neste disco.
    let tracked: BTreeSet<String> =
        mustard_core::platform::git_exclude::tracked_paths(root, &cited).into_iter().collect();
    for task in &ahead {
        let code = code_of(task);
        for (path, new) in declared_files(task) {
            if !new && !tracked.contains(&path) {
                out.push(PlanFinding::FileOutsideGit { task: code.clone(), path });
            }
        }
    }

    // A cobertura: o item de uma onda sem tarefa e o contrato sem critério só
    // avisam, e a decisão fica com quem aprova. O item sem dono não é
    // conferido aqui: a análise antes do envio de cada onda decide se ele vai
    // para ela.
    let covered: BTreeSet<u64> = tasks
        .iter()
        .flat_map(|task| task.ints("covers"))
        .filter_map(|id| log.current(id).map(|e| e.id))
        .collect();
    let agreed: Vec<&SpecEvent> = log
        .block(BlockQuery::Block(Block::Agreed))
        .into_iter()
        .filter(|e| e.str_field("text").is_some_and(|t| !t.trim().is_empty()))
        .collect();
    // O item do projeto é regra que vale sempre, sem tarefa que o implemente;
    // o marcado como "não vira código" traz o motivo na própria linha. Avisar
    // sobre eles seria avisar para sempre.
    let owners = wave_prompt::owners(log);
    for item in &agreed {
        match owners.get(&item.id) {
            None | Some(Owner::Project) => {}
            Some(Owner::Waves(_)) => {
                if !covered.contains(&item.id) && item.str_field("no_code").is_none() {
                    out.push(PlanFinding::ItemWithoutTask { code: code_of(item) });
                }
            }
        }
    }
    // Cada tarefa casa com a onda em que está: a nota dessa onda contra as
    // das outras, pela mesma busca do recorte dos itens. A pergunta é se a
    // tarefa pertence à onda dela, e tem resposta; qual das ondas casaria mais
    // forte é outra pergunta, sempre tem um vencedor e recusaria quase tudo,
    // então só entra no aviso, para dizer para onde a tarefa iria. Esse aviso
    // não trava: medido contra uma spec real, ele apontava uma em cada seis
    // tarefas que estavam no lugar certo e deixava passar quatro em cada dez
    // postas fora do lugar. A tarefa que não casa com onda nenhuma não tem
    // destino a apontar, e essa trava.
    for task in &ahead {
        let Some(mine) = task.int("wave") else { continue };
        let text = task.str_field("text").unwrap_or_default();
        if wave_prompt::matches_wave(log, mine, text) {
            continue;
        }
        out.push(match wave_prompt::closest_wave(log, text) {
            Some(best) => PlanFinding::TaskInTheWrongWave { task: code_of(task), wave: mine, best },
            None => PlanFinding::TaskMatchesNoWave { task: code_of(task), wave: mine },
        });
    }

    // A skill nasce por demanda e é escolhida pela tarefa: a tarefa que não
    // nomeia skill ganha o nome da que já existe e serve; quando nenhuma
    // serve e o trabalho dela se repete no projeto, o plano precisa da tarefa
    // que faz a skill nascer. Como as outras conferências das tarefas, esta
    // olha só as ondas que ainda vêm.
    let on_disk = skills_on_disk(root, &tasks);
    for task in &ahead {
        if task.str_field("skill").is_some_and(|s| !s.trim().is_empty()) {
            continue;
        }
        let text = task.str_field("text").unwrap_or_default();
        match best_skill(&on_disk, text) {
            Some(name) => {
                out.push(PlanFinding::TaskCouldNameASkill { task: code_of(task), skill: name });
            }
            None if repeats_in_the_project(root, task) => {
                out.push(PlanFinding::SkillToBeBorn { task: code_of(task) });
            }
            None => {}
        }
    }

    let with_criterion: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Criteria))
        .into_iter()
        .flat_map(|e| e.ints("contracts"))
        .filter_map(|id| log.current(id).map(|e| e.id))
        .collect();
    for contract in agreed.iter().filter(|e| e.event_type == "contract") {
        if !with_criterion.contains(&contract.id) {
            out.push(PlanFinding::ContractWithoutCriterion { code: code_of(contract) });
        }
    }
    out
}

/// `true` quando o trabalho de uma tarefa se repete no projeto: o mapa acha
/// arquivos do mesmo tipo dos que ela mexe. É o sinal de que vale uma skill.
fn repeats_in_the_project(root: &Path, task: &SpecEvent) -> bool {
    let Some((target, _)) = declared_files(task).into_iter().next() else { return false };
    let Ok(map) = mustard_core::io::project_map::read(root) else { return false };
    !mustard_core::domain::project_map::examples(&map, &target, Locale::PtBr).picks.is_empty()
}

/// As frases com que uma tarefa declara, no texto, que não mexe em arquivo
/// nenhum — a de prosa, a de decisão, a de medida e a que só escreve na spec.
/// Essa tarefa pode vir sem o campo dos arquivos; qualquer outra é recusada.
const TOUCHES_NO_FILE: &[&str] = &[
    "nao mexe em arquivo",
    "nao altera arquivo",
    "nao mexe em nenhum arquivo",
    "nao altera nenhum arquivo",
    "does not change any file",
    "does not touch any file",
    "changes no file",
];

/// `true` quando o texto da tarefa diz, com todas as letras, que ela não mexe
/// em arquivo.
fn says_it_touches_no_file(text: &str) -> bool {
    let folded = mustard_core::domain::text::fold(text);
    TOUCHES_NO_FILE.iter().any(|phrase| folded.contains(phrase))
}

/// Os arquivos que uma tarefa declara: o caminho e se ela o marcou como novo.
pub(super) fn declared_files(task: &SpecEvent) -> Vec<(String, bool)> {
    task.fields
        .get("files")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|file| {
            let path = file.as_str().or_else(|| file.get("path").and_then(Value::as_str))?;
            let path = path.trim().replace('\\', "/");
            (!path.is_empty())
                .then(|| (path, file.get("new").and_then(Value::as_bool) == Some(true)))
        })
        .collect()
}

fn join(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(", ")
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::flow::grill::{grill_for, GrillOpts};
    use crate::commands::spec_events::pages::copy::{sent, sent_items};
    use crate::commands::spec_events::write::{record_open, seed_at, WriteOpts};
    use std::process::Command;
    use tempfile::tempdir;

    const GOAL: &str = "Travar o merge enquanto houver pendência aberta.";

    fn write(root: &Path, spec: Option<&str>, event_type: &str, body: Value) -> Value {
        seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: spec.map(str::to_string),
            event_type: event_type.into(),
            json: with_task_declarations(event_type, body).to_string(),
        })
    }

    /// Os testes deste arquivo escrevem a tarefa pelo que ela importa para
    /// eles (pontos, arquivos, ondas); as duas declarações que a gravação
    /// agora exige sempre (`files`, `depends_on`) entram vazias quando o
    /// teste não as deu, sem mudar o que ele já afirma.
    fn with_task_declarations(event_type: &str, mut body: Value) -> Value {
        if event_type == "task" {
            let map = body.as_object_mut().expect("a tarefa é um objeto");
            map.entry("files").or_insert_with(|| json!([]));
            map.entry("depends_on").or_insert_with(|| json!([]));
        }
        body
    }

    fn id_of(report: &Value) -> u64 {
        report["id"].as_u64().unwrap_or_else(|| panic!("não gravou: {report}"))
    }

    fn git(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Um projeto com dois arquivos no git e uma spec cujo levantamento
    /// terminou: todos os pontos fechados, pronta para o plano.
    fn surveyed(root: &Path, spec: &str) -> u64 {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        for name in ["a.rs", "b.rs"] {
            std::fs::write(root.join("src").join(name), "fn um() {}\nfn dois() {}\n").unwrap();
        }
        git(root, &["init", "-q"]);
        git(root, &["add", "src"]);
        git(root, &["commit", "-q", "-m", "semente"]);

        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, Some(spec), "message", json!({"author": "user", "text": GOAL})));
        id_of(&write(root, Some(spec), "context", json!({"text": GOAL, "origin": said})));
        let opts = GrillOpts { root: root.to_path_buf(), spec: Some(spec.into()), kinds: Some("fix".into()), condensed: false };
        let listed = grill_for(&opts, None);
        for item in listed["points"].as_array().cloned().unwrap_or_default() {
            let mut point = item.clone();
            point["status"] = json!("open");
            point["facts"] = json!([{"text": GOAL, "source": "src/a.rs:1"}]);
            let opened = write(root, Some(spec), "point", point);
            let closing = json!({"block": item["block"], "gap": item["gap"], "from": "gap",
                "status": "not_applicable", "closes": id_of(&opened),
                "reason": "Já respondido.", "origin": said});
            assert_eq!(write(root, Some(spec), "point", closing)["ok"], json!(true));
        }
        said
    }

    /// Um critério da spec, que toda onda precisa apontar.
    fn criterion(root: &Path, spec: &str, said: u64) -> u64 {
        id_of(&write(root, Some(spec), "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test", "form": "ubiquitous",
                "origin": said})))
    }

    /// Uma onda com uma tarefa, num plano que passa em tudo.
    fn sound_plan(root: &Path, spec: &str, said: u64) {
        let crit = criterion(root, spec, said);
        write(root, Some(spec), "wave", json!({"n": 1, "text": "Escrever a soma.", "criteria": [crit], "done_when": "A suíte passa.", "origin": said}));
        write(root, Some(spec), "task", json!({"wave": 1, "text": "Escrever a soma.", "files": [{"path": "src/a.rs"}], "origin": said}));
    }

    fn plan(root: &Path, spec: &str) -> Value {
        plan_for(&PlanOpts { root: root.to_path_buf(), spec: Some(spec.into()) }, None)
    }

    fn reasons(report: &Value, field: &str) -> Vec<String> {
        report[field]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|f| f["reason"].as_str().map(str::to_string))
            .collect()
    }

    /// Um plano são é conferido numa chamada: a spec passa para o plano, a
    /// cópia para o banco da página sai preparada, sem página nem `.md`
    /// escritos, e a resposta manda publicar as duas páginas, copiar os lotes
    /// e fazer a pergunta.
    #[test]
    fn a_sound_plan_is_checked_in_one_call_and_asks_for_the_page_and_the_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["phase"], json!("plan"));
        assert_eq!(report["from"], json!("survey"));
        assert_eq!(report["waves"][0]["wave"], json!(1));
        assert!(report["waves"][0]["lines"].as_u64().unwrap() > 0);
        let next = report["next"].as_str().unwrap_or_default();
        // A pergunta de aprovação sai com o texto exato que a testemunha da
        // aprovação reconhece, e nenhuma vaga fica por preencher.
        assert!(next.contains("com o texto exato \"Aprovar esta spec?\""), "{next}");
        assert!(!next.contains("{question}"), "{next}");
        for page in ["spec", "project"] {
            assert!(next.contains(&format!(r#"'{{"page":"{page}","milestone":"approval","#)), "{page}: {next}");
        }
        assert!(next.contains("write copy") && next.contains("ArtifactData"), "{next}");
        // A aprovação é um marco: a resposta manda publicar as duas páginas,
        // que ainda não têm endereço, e copiar a spec inteira para o banco.
        assert_eq!(report["publish"], json!(["spec", "project"]), "{report}");
        assert!(!report.to_string().contains("http"), "{report}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(!sent_items(root, &report).is_empty(), "{report}");
        assert_eq!(report["copy"]["spec"]["record"], json!({"page": "spec", "last": log.max_id()}), "{report}");
        for page in ["spec.html", "spec.md"] {
            assert!(!root.join(".claude/spec/x").join(page).exists(), "{page}");
        }
        assert!(!root.join(".claude/spec/project.html").exists());

        // A linha da spec no índice sai refeita no passo.
        let index = std::fs::read_to_string(root.join(".claude/spec/index.ndjson")).unwrap();
        assert!(index.contains("\"phase\":\"plan\""), "{index}");

        // Rodar de novo não grava fase nenhuma, refaz o índice e continua
        // respondendo o mesmo.
        std::fs::remove_file(root.join(".claude/spec/index.ndjson")).unwrap();
        let again = plan(root, "x");
        assert_eq!(again["ok"], json!(true), "{again}");
        assert_eq!(again["from"], json!("plan"));
        assert!(again["id"].is_null(), "{again}");
        assert_eq!(again["publish"], json!(["spec", "project"]), "{again}");
        let rebuilt = std::fs::read_to_string(root.join(".claude/spec/index.ndjson")).unwrap();
        assert_eq!(rebuilt, index, "o índice volta igual");
    }

    /// Cada linha de cada pedido aparece na página da spec que o comando de
    /// página refaz, sem exceção, as instruções fixas incluídas, e vai inteiro
    /// para o banco da página publicada no documento das coisas calculadas: é
    /// por essa página que a spec é aprovada.
    #[test]
    fn every_line_of_every_request_shows_up_on_the_page() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "rule", json!({"text": "No máximo 3 tentativas de compilação.",
            "example": "a quarta para", "keys": ["tentativas"], "applies_to": {"files": ["**"]}, "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Escrever a soma.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Escrever a soma.", "files": [{"path": "src/a.rs"}], "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 2, "text": "Escrever a subtração.", "criteria": [crit], "done_when": "passa", "depends_on": [1], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Escrever a subtração.", "files": [{"path": "src/b.rs"}], "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "{report}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let built = prompts(root, "x", &log, Locale::PtBr, &Flight::default());
        assert_eq!(built.len(), 2, "duas ondas, dois pedidos");
        let computed = sent(root, &report, "spec").into_iter().find(|w| w["collection"] == json!("computed")).unwrap();
        for prompt in &built {
            assert!(prompt.lines > 0);
            // O texto que o banco da página recebe é o mesmo que o agente lê,
            // instruções fixas incluídas: nenhuma linha fica de fora.
            assert_eq!(computed["body"]["prompts"][prompt.wave.to_string()], json!(prompt.text), "{computed}");
        }
        assert!(built.iter().any(|prompt| prompt.text.contains(translate("prompt.fixed", Locale::PtBr).lines().next().unwrap())));
    }

    /// Um ponto do levantamento ainda aberto trava a pergunta de aprovação, e
    /// a spec não passa para o plano.
    #[test]
    fn an_open_survey_point_blocks_the_approval_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);
        write(root, Some("x"), "point", json!({"block": "limits", "gap": "o teto", "from": "gap",
            "status": "open", "facts": [{"text": GOAL, "source": "src/a.rs:1"}], "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        assert_eq!(report["reason"], json!("plan-not-ready"));
        assert!(reasons(&report, "blocking").contains(&"survey-open".to_string()), "{report}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(State::from_log(&log).phase, Some("survey"), "a fase não andou");
    }

    /// Os erros de montagem do plano travam a pergunta: o ciclo entre ondas, a
    /// dependência que aponta uma onda que não existe e a tarefa de uma onda
    /// que não existe.
    #[test]
    fn a_plan_that_does_not_assemble_blocks_the_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Uma.", "criteria": [crit], "done_when": "passa", "depends_on": [2], "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 2, "text": "Outra.", "criteria": [crit], "done_when": "passa", "depends_on": [1, 9], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 7, "text": "Perdida.", "files": [{"path": "src/a.rs"}], "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        let blocking = reasons(&report, "blocking");
        for reason in ["waves-loop", "depends-on-missing-wave", "task-without-wave"] {
            assert!(blocking.contains(&reason.to_string()), "{reason}: {report}");
        }
    }

    /// Uma linha crua, direto no arquivo da spec, sem passar pela gravação:
    /// simula uma onda grande que já existia antes da onda nascer pequena, ou
    /// uma edição feita fora do binário — o caso que a rede do plano cobre.
    fn append_raw(root: &Path, spec: &str, event_type: &str, body: Value, id: u64) {
        let mut map = mustard_core::domain::spec_events::normalize(
            body.as_object().cloned().unwrap_or_default(),
            event_type,
        );
        map.insert("type".into(), json!(event_type));
        let line = mustard_core::domain::spec_events::render_line(
            &mustard_core::domain::spec_events::stamp(map, id, None, "2026-09-20T10:00:00-03:00"),
        );
        use std::io::Write as _;
        let path = store::spec_file(root, spec).unwrap();
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(file, "{line}").unwrap();
    }

    /// Um pedido bem além do antigo teto de 500 linhas não trava mais a
    /// pergunta: o teto não existe. As 600 tarefas vêm direto no arquivo,
    /// sem passar pela gravação, porque a onda nasce pequena agora e uma
    /// onda com tantas tarefas não seria mais gravada — mas o plano continua
    /// conferindo a que já existe no arquivo, de antes da regra ou de uma
    /// edição de fora.
    #[test]
    fn a_request_far_past_the_old_line_cap_does_not_block_the_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        let wave = write(root, Some("x"), "wave", json!({"n": 1, "text": "Somar.", "criteria": [crit], "done_when": "passa", "origin": said}));
        // Os códigos das tarefas cabem numa linha só: o que passa do teto
        // antigo é uma parte de uma linha por item, como a das skills que
        // elas nomeiam.
        let first_id = id_of(&wave) + 1;
        for i in 0..600 {
            let skill = root.join(".claude").join("skills").join(format!("s{i}"));
            std::fs::create_dir_all(&skill).unwrap();
            std::fs::write(skill.join("SKILL.md"), format!("# s{i}\n")).unwrap();
            append_raw(root, "x", "task", json!({"wave": 1, "text": "Somar.", "files": [{"path": "src/a.rs"}],
                "skill": format!("s{i}"), "origin": said}), first_id + i);
        }

        let report = plan(root, "x");
        assert!(!reasons(&report, "blocking").contains(&"wave-prompt-too-long".to_string()), "{report}");
        assert!(report["waves"][0]["lines"].as_u64().unwrap() > 500, "{report}");
    }

    /// O plano não tem mais rede de tamanho: uma onda com mais de três
    /// tarefas no arquivo — nascida assim antes da regra sair, ou por uma
    /// edição de fora do binário — passa sem bloqueio nenhum. É o caso que a
    /// recusa `wave-too-big` bloqueava antes desta onda; quem corta o custo
    /// agora é o teto de turnos do agente, fora do plano.
    #[test]
    fn a_wave_grown_big_outside_the_write_is_not_blocked_by_the_plan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        let wave = write(root, Some("x"), "wave",
            json!({"n": 1, "text": "Somar.", "criteria": [crit], "done_when": "passa", "origin": said}));
        let first_id = id_of(&wave) + 1;
        for i in 0..4 {
            append_raw(root, "x", "task", json!({"wave": 1, "text": format!("Tarefa {i}."),
                "files": [{"path": "src/a.rs"}], "origin": said}), first_id + i);
        }

        let report = plan(root, "x");
        assert!(!reasons(&report, "blocking").contains(&"wave-too-big".to_string()), "{report}");
    }

    /// As citações do plano passam pela mesma conferência do ponto do
    /// levantamento: o arquivo que não existe e não está marcado como novo
    /// trava; o marcado como novo passa; e o nome que o mapa não acha avisa.
    #[test]
    fn the_plan_citations_go_through_the_same_check_as_a_survey_point() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Somar.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Some com o `Somador`.",
            "files": [{"path": "src/nao-existe.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Escreve o resto.",
            "files": [{"path": "src/novo.rs", "new": true}], "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        let blocking = reasons(&report, "blocking");
        assert_eq!(blocking.iter().filter(|r| *r == "cited-file-missing").count(), 1, "{report}");
        assert!(reasons(&report, "warnings").contains(&"names-unchecked".to_string()), "{report}");
    }

    /// Só avisam, e a pergunta segue: arquivo fora do git, ondas da mesma
    /// rodada dividindo arquivo, item de uma onda sem tarefa e contrato sem
    /// critério. A
    /// tarefa que diz no texto que não mexe em arquivo passa sem o campo.
    #[test]
    fn the_advisory_findings_never_block_the_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        std::fs::write(root.join("src/fora.rs"), "fn tres() {}\n").unwrap();
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "contract", json!({"text": "A barra tem duas linhas.", "example": "dev · x", "keys": ["barra"], "waves": [1], "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Mexer no código.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer.", "files": [{"path": "src/a.rs"}, {"path": "src/fora.rs"}], "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 2, "text": "Mexer na spec.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Mexer também.", "files": [{"path": "src/a.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Não mexe em arquivo: é escrita na spec.", "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "{report}");
        let warnings = reasons(&report, "warnings");
        for reason in ["file-outside-git", "waves-share-a-file", "item-without-task", "contract-without-criterion"] {
            assert!(warnings.contains(&reason.to_string()), "{reason}: {report}");
        }
        assert!(!warnings.contains(&"task-without-file".to_string()), "{report}");
    }

    /// O comando provisório que `mustard init` grava em `buildCommand`,
    /// quando o projeto não tem um stack reconhecido, não chega ao pedido da
    /// onda: `commands()` o trata como ausente, então nem o texto do
    /// provisório nem a linha "Compile com" aparecem, e o plano avisa, sem
    /// recusar, qual campo falta preencher — só o campo mesmo ausente, não o
    /// que está declarado.
    #[test]
    fn a_placeholder_command_never_reaches_the_request() {
        for (build, test, missing, present) in [
            (mustard_core::BUILD_COMMAND_FALLBACK, "make test", "buildCommand", "testCommand"),
            ("cargo build", "", "testCommand", "buildCommand"),
        ] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let said = surveyed(root, "x");
            std::fs::write(
                root.join("mustard.json"),
                json!({"build_command": build, "test_command": test}).to_string(),
            )
            .unwrap();
            sound_plan(root, "x", said);

            let report = plan(root, "x");
            assert_eq!(report["ok"], json!(true), "{report}");
            let hints = hints_of(&report, "warnings", "command-not-declared");
            assert!(hints.iter().any(|h| h.contains(missing)), "{missing}: {hints:?}");
            assert!(!hints.iter().any(|h| h.contains(present)), "{present} está declarado: {hints:?}");

            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let built = prompts(root, "x", &log, Locale::PtBr, &Flight::default());
            assert_eq!(built.len(), 1, "{built:?}");
            let text = &built[0].text;
            assert!(!text.contains(mustard_core::BUILD_COMMAND_FALLBACK), "{text}");
            assert_eq!(text.contains("Compile com"), missing != "buildCommand", "{text}");
            assert_eq!(text.contains("Teste com"), missing != "testCommand", "{text}");
        }
    }

    /// A onda cujas tarefas não dividem arquivo entre si tem partes
    /// independentes: o plano diz quais são e que ela sai dividida, uma onda
    /// por parte, em paralelo — e só avisa, nunca segura a pergunta. A onda
    /// cujas tarefas se tocam pelo mesmo arquivo é uma parte só, e sobre ela
    /// o plano não diz nada.
    #[test]
    fn a_wave_with_independent_parts_is_told_to_go_out_split() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Mexer no código.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer no código de um.",
            "files": [{"path": "src/a.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer no código de dois.",
            "files": [{"path": "src/a.rs"}, {"path": "src/b.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer no código de três.",
            "files": [{"path": "src/c.rs", "new": true}], "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 2, "text": "Mexer na página.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Mexer na página de um.",
            "files": [{"path": "src/d.rs", "new": true}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Mexer na página de dois.",
            "files": [{"path": "src/d.rs", "new": true}], "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "o aviso não segura a pergunta: {report}");
        let split = hints_of(&report, "warnings", "wave-should-split");
        assert_eq!(split.len(), 1, "só a onda de duas partes é avisada: {report}");
        let expected = translate("plan.wave_should_split", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{parts}", "MSTD-TASK-0001 + MSTD-TASK-0002; MSTD-TASK-0003");
        assert_eq!(split[0], expected, "{split:?}");
        assert!(!split[0].contains("MSTD-TASK-0004"), "a onda de uma parte só não entra: {split:?}");
    }

    /// A onda que já tem registro de entrega não é avisada para sair
    /// dividida: o que ela fez já entrou, e dividir agora não divide nada. A
    /// onda de partes independentes que ainda vem, ao lado dela, continua
    /// avisada.
    #[test]
    fn a_delivered_wave_is_not_told_to_go_out_split() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        for n in [1, 2] {
            write(root, Some("x"), "wave", json!({"n": n, "text": "Mexer no código.", "criteria": [crit],
                "done_when": "passa", "origin": said}));
            write(root, Some("x"), "task", json!({"wave": n, "text": "Mexer no código de um.",
                "files": [{"path": format!("src/a{n}.rs"), "new": true}], "origin": said}));
            write(root, Some("x"), "task", json!({"wave": n, "text": "Mexer no código de dois.",
                "files": [{"path": format!("src/b{n}.rs"), "new": true}], "origin": said}));
        }
        let record = write(root, Some("x"), "delivered",
            json!({"wave": 1, "text": "A onda 1 saiu.", "files": ["src/a1.rs", "src/b1.rs"]}));
        assert_eq!(record["ok"], json!(true), "{record}");

        let report = plan(root, "x");
        let split = hints_of(&report, "warnings", "wave-should-split");
        assert_eq!(split.len(), 1, "só a onda que ainda vem é avisada: {report}");
        let expected = translate("plan.wave_should_split", Locale::PtBr)
            .replace("{wave}", "2")
            .replace("{parts}", "MSTD-TASK-0003; MSTD-TASK-0004");
        assert_eq!(split[0], expected, "{split:?}");
    }

    /// A spec cujas tarefas, em ondas diferentes, não dividem arquivo entre
    /// si tem partes independentes: o plano avisa, pela mesma conta que avisa
    /// a onda, que ela pode ser dividida, e a pergunta segue. Nenhuma onda
    /// aqui tem duas partes, então o aviso é só o da spec. Na divisa: com duas
    /// partes o aviso sai; a tarefa que liga as duas por um arquivo em comum
    /// deixa a spec com uma parte só, e o aviso some.
    #[test]
    fn a_spec_with_independent_parts_across_waves_is_told_it_can_be_split() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        for (n, depends_on) in [(1, json!([])), (2, json!([1])), (3, json!([2]))] {
            write(root, Some("x"), "wave", json!({"n": n, "text": "Mexer no código.", "criteria": [crit],
                "done_when": "passa", "depends_on": depends_on, "origin": said}));
        }
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer no código de um.",
            "files": [{"path": "src/a.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Mexer no código de dois.",
            "files": [{"path": "src/b.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 3, "text": "Mexer no código de três.",
            "files": [{"path": "src/a.rs"}, {"path": "src/c.rs", "new": true}], "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "o aviso não segura a pergunta: {report}");
        assert!(hints_of(&report, "blocking", "spec-should-split").is_empty(), "{report}");
        assert!(hints_of(&report, "warnings", "wave-should-split").is_empty(), "nenhuma onda tem duas partes: {report}");
        let split = hints_of(&report, "warnings", "spec-should-split");
        let expected = translate("plan.spec_should_split", Locale::PtBr)
            .replace("{parts}", "MSTD-TASK-0001 + MSTD-TASK-0003; MSTD-TASK-0002");
        assert_eq!(split, vec![expected], "as partes se juntam pelo arquivo, não pela onda: {report}");

        // A tarefa que toca os dois arquivos junta as duas partes numa só.
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Mexer no código de um e de dois.",
            "files": [{"path": "src/a.rs"}, {"path": "src/b.rs"}], "origin": said}));
        let joined = plan(root, "x");
        assert_eq!(joined["ok"], json!(true), "{joined}");
        assert!(hints_of(&joined, "warnings", "spec-should-split").is_empty(), "uma parte só não é avisada: {joined}");
    }

    /// A onda já entregue não entra na conta da spec: o que ela fez já está
    /// na spec e não sai para outra. As duas ondas que ainda vêm, que só a
    /// entregue ligava por arquivo, são duas partes, e o aviso sai.
    #[test]
    fn a_delivered_wave_does_not_join_the_parts_of_the_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        for (n, depends_on) in [(1, json!([])), (2, json!([1])), (3, json!([2]))] {
            write(root, Some("x"), "wave", json!({"n": n, "text": "Mexer no código.", "criteria": [crit],
                "done_when": "passa", "depends_on": depends_on, "origin": said}));
        }
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer no código de um e de dois.",
            "files": [{"path": "src/a.rs"}, {"path": "src/b.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Mexer no código de um.",
            "files": [{"path": "src/a.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 3, "text": "Mexer no código de dois.",
            "files": [{"path": "src/b.rs"}], "origin": said}));
        let before = plan(root, "x");
        assert!(hints_of(&before, "warnings", "spec-should-split").is_empty(), "a onda 1 liga as outras: {before}");

        let record = write(root, Some("x"), "delivered",
            json!({"wave": 1, "text": "A onda 1 saiu.", "files": ["src/a.rs", "src/b.rs"]}));
        assert_eq!(record["ok"], json!(true), "{record}");
        let after = plan(root, "x");
        let expected = translate("plan.spec_should_split", Locale::PtBr)
            .replace("{parts}", "MSTD-TASK-0002; MSTD-TASK-0003");
        assert_eq!(hints_of(&after, "warnings", "spec-should-split"), vec![expected], "{after}");
    }

    /// A tarefa que não nomeia skill ganha o nome da skill que já existe e
    /// serve para ela; a que não tem skill nenhuma que sirva, e cujo trabalho
    /// se repete no projeto, pede a tarefa que faz a skill nascer.
    #[test]
    fn the_plan_names_the_skill_that_serves_and_asks_for_the_one_that_is_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        let skill = root.join(".claude/skills/add-run-command");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: add-run-command\ndescription: Use quando for preciso adicionar um comando \
             de execução novo, com os registros, a recusa nos dois idiomas e os testes.\n---\n\nPassos.\n",
        )
        .unwrap();
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Uma.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Adicionar um comando de execução novo, com os registros e os testes.",
            "files": [{"path": "src/a.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Já tem skill.", "skill": "add-run-command",
            "files": [{"path": "src/b.rs"}], "origin": said}));

        let report = plan(root, "x");
        let named: Vec<&Value> = report["warnings"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|f| f["reason"] == json!("task-could-name-a-skill"))
            .collect();
        assert_eq!(named.len(), 1, "só a tarefa sem skill recebe o nome: {report}");
        let hint = named[0]["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("MSTD-TASK-0001") && hint.contains("add-run-command"), "{hint}");
    }

    /// Os achados de um motivo, pela mensagem, num dos campos da resposta.
    fn hints_of(report: &Value, field: &str, reason: &str) -> Vec<String> {
        report[field]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|f| f["reason"] == json!(reason))
            .filter_map(|f| f["hint"].as_str().map(str::to_string))
            .collect()
    }

    /// A tarefa cujo texto não casa com o texto da onda dela gera um aviso,
    /// que diz com qual onda ele casaria melhor, e o plano segue. A que casa
    /// com a onda dela não gera nada, mesmo quando o texto de outra onda casa
    /// mais forte: a pergunta é se a tarefa pertence à onda dela, não qual das
    /// ondas vence.
    #[test]
    fn a_task_that_does_not_match_its_own_wave_is_warned_about_and_says_where_it_would_go() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Os ganchos da sessão: bloquear, avisar e injetar texto.",
            "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 2, "text": "A página da spec: o desenho, os blocos e a publicação.",
            "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 3, "text": "O instalador: semear o projeto e as permissões.",
            "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "O gancho que bloqueia a gravação avisa o motivo.",
            "files": [{"path": "src/a.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "A publicação da página da spec sai no fim do passo.",
            "files": [{"path": "src/b.rs"}], "origin": said}));
        let torn = "O gancho avisa que a publicação da página da spec saiu.";
        write(root, Some("x"), "task", json!({"wave": 1, "text": torn, "files": [{"path": "src/a.rs"}], "origin": said}));

        // A terceira tarefa casa com a onda dela, e a onda que casa mais forte
        // com ela é outra.
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(wave_prompt::matches_wave(&log, 1, torn), "a onda dela responde que casa");
        assert_eq!(wave_prompt::closest_wave(&log, torn), Some(2), "a onda mais forte é outra");

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "o aviso não trava o plano: {report}");
        assert!(hints_of(&report, "blocking", "task-in-the-wrong-wave").is_empty(), "{report}");
        let wrong = hints_of(&report, "warnings", "task-in-the-wrong-wave");
        assert_eq!(wrong.len(), 1, "só a tarefa que não casa com a onda dela é apontada: {report}");
        assert!(wrong[0].contains("MSTD-TASK-0002"), "{wrong:?}");
        assert!(wrong[0].contains("onda 2"), "o aviso diz para onde a tarefa iria: {wrong:?}");
    }

    /// A tarefa cujo texto não casa com onda nenhuma do plano trava o plano, e
    /// a recusa não aponta destino, porque não há para onde ela ir.
    #[test]
    fn a_task_that_matches_no_wave_blocks_the_plan_without_a_destination() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Os ganchos da sessão: bloquear e avisar.",
            "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 2, "text": "A página da spec e a publicação.",
            "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "O gancho avisa o motivo.",
            "files": [{"path": "src/a.rs"}], "origin": said}));
        let lost = "Somar dois números inteiros.";
        write(root, Some("x"), "task", json!({"wave": 2, "text": lost, "files": [{"path": "src/b.rs"}], "origin": said}));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(wave_prompt::closest_wave(&log, lost), None, "o texto não casa com onda nenhuma");

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        let refused = hints_of(&report, "blocking", "task-matches-no-wave");
        assert_eq!(refused.len(), 1, "só a tarefa sem onda que case é recusada: {report}");
        let expected = translate("plan.task_matches_no_wave", Locale::PtBr)
            .replace("{task}", "MSTD-TASK-0002")
            .replace("{wave}", "2");
        assert_eq!(refused[0], expected, "a recusa não traz destino");
        assert!(hints_of(&report, "warnings", "task-in-the-wrong-wave").is_empty(), "{report}");
    }

    /// O aviso de que uma tarefa podia nomear uma skill olha só as ondas que
    /// ainda vêm: a mesma tarefa sem skill avisa na onda sem registro de
    /// entrega e se cala na onda entregue.
    #[test]
    fn the_skill_warning_looks_only_at_the_waves_still_to_come() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        let skill = root.join(".claude/skills/add-run-command");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: add-run-command\ndescription: Use quando for preciso adicionar um comando \
             de execução novo, com os registros, a recusa nos dois idiomas e os testes.\n---\n\nPassos.\n",
        )
        .unwrap();
        for n in [1, 2] {
            write(root, Some("x"), "wave", json!({"n": n, "text": "Os comandos de execução novos.",
                "criteria": [crit], "done_when": "passa", "origin": said}));
            write(root, Some("x"), "task", json!({"wave": n, "text": "Adicionar um comando de execução novo, com os registros e os testes.",
                "files": [{"path": "src/a.rs"}], "origin": said}));
        }
        let record = write(root, Some("x"), "delivered", json!({"wave": 1, "text": "A onda 1 saiu.", "files": ["src/a.rs"]}));
        assert_eq!(record["ok"], json!(true), "{record}");

        let report = plan(root, "x");
        let named = hints_of(&report, "warnings", "task-could-name-a-skill");
        assert_eq!(named.len(), 1, "só a onda que ainda vem avisa: {report}");
        assert!(named[0].contains("MSTD-TASK-0002"), "{named:?}");
    }

    /// A tarefa de onda que já tem registro de entrega não é conferida: o
    /// arquivo citado, a tarefa sem arquivo e a coerência olham só as ondas
    /// que ainda vêm.
    #[test]
    fn a_task_of_a_delivered_wave_is_not_checked_anymore() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Os ganchos da sessão.",
            "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 2, "text": "A página da spec.",
            "criteria": [crit], "done_when": "passa", "origin": said}));
        // A mesma tarefa quebrada nas duas ondas: cita um arquivo que não
        // existe e não está marcado como novo.
        for wave in [1, 2] {
            write(root, Some("x"), "task", json!({"wave": wave, "text": "Somar dois números.",
                "files": [{"path": "src/somar.rs"}], "origin": said}));
        }
        let record = write(root, Some("x"), "delivered",
            json!({"wave": 1, "text": "A onda 1 saiu.", "files": ["src/somar.rs"]}));
        assert_eq!(record["ok"], json!(true), "{record}");

        let report = plan(root, "x");
        let blocking: Vec<String> = report["blocking"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|f| f["hint"].as_str().map(str::to_string))
            .collect();
        assert!(blocking.iter().any(|h| h.contains("MSTD-TASK-0002")), "a onda que ainda vem é conferida: {report}");
        assert!(!blocking.iter().any(|h| h.contains("MSTD-TASK-0001")), "a onda entregue não é: {report}");
    }

    /// O item combinado marcado como "não vira código", com o motivo na
    /// própria linha, some do aviso dos itens sem tarefa; o item igual sem a
    /// marca continua avisando.
    #[test]
    fn an_item_marked_as_not_becoming_code_stops_being_warned_about() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "decision", json!({"text": "Quem decidiu sozinho.", "keys": ["registro"],
            "why": "registro da conversa", "no_code": "é registro de processo, não vira código", "waves": [1], "origin": said}));
        write(root, Some("x"), "decision", json!({"text": "A rodada formata os arquivos dela.", "keys": ["formatador"],
            "why": "o commit sai formatado", "waves": [1], "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Uma.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer.", "files": [{"path": "src/a.rs"}], "origin": said}));

        let report = plan(root, "x");
        let uncovered: Vec<String> = report["warnings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|f| f["reason"] == json!("item-without-task"))
            .filter_map(|f| f["hint"].as_str().map(str::to_string))
            .collect();
        assert_eq!(uncovered.len(), 1, "só o item sem a marca avisa: {report}");
        assert!(uncovered[0].contains("MSTD-DEC-0002"), "{uncovered:?}");
    }

    /// O item combinado sem dono não trava o plano nem avisa: a análise antes
    /// do envio de cada onda decide se ele vai para ela. Sem dono ficam o item
    /// que nada cobre e o que diz uma onda que o plano não tem. Os outros
    /// seguem como eram: o item que a tarefa cobre e o do projeto todo não
    /// avisam, e o que diz a onda dele sem tarefa que o cubra avisa. Na divisa,
    /// o plano continua travando o que trava: a tarefa sem arquivo segura a
    /// pergunta, e o item sem dono não aparece entre os motivos.
    #[test]
    fn the_plan_accepts_an_item_without_owner() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        let decision = |text: &str, extra: Value| {
            let mut body = json!({"text": text, "keys": ["k"], "why": "w", "origin": said});
            body.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
            id_of(&write(root, Some("x"), "decision", body))
        };
        decision("Sem dono.", json!({}));
        let covered = decision("Coberta.", json!({}));
        decision("Da onda um.", json!({"waves": [1]}));
        decision("Do projeto.", json!({"applies_to": {"files": ["**"]}}));
        decision("Da onda que não existe.", json!({"waves": [9]}));
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Mexer no código.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer.", "files": [{"path": "src/a.rs"}],
            "covers": [covered], "origin": said}));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let unowned: Vec<String> = wave_prompt::unowned(&log).iter().map(|e| codes[&e.id].clone()).collect();
        assert_eq!(unowned, ["MSTD-DEC-0001", "MSTD-DEC-0005"], "the two items really have no owner");

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "the item without owner does not hold the plan: {report}");
        assert_eq!(report["phase"], json!("plan"), "{report}");
        let every: Vec<String> = ["blocking", "warnings"]
            .iter()
            .flat_map(|field| report[*field].as_array().cloned().unwrap_or_default())
            .map(|f| f.to_string())
            .collect();
        for code in &unowned {
            assert!(every.iter().all(|f| !f.contains(code.as_str())), "{code} is not a finding: {report}");
        }
        let uncovered = hints_of(&report, "warnings", "item-without-task");
        assert_eq!(uncovered.len(), 1, "só a da onda um avisa, o do projeto não: {report}");
        assert!(uncovered[0].contains("MSTD-DEC-0003"), "{uncovered:?}");

        // O que trava continua travando, e o item sem dono não entra na conta.
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer mais.", "origin": said}));
        let held = plan(root, "x");
        assert_eq!(held["ok"], json!(false), "{held}");
        assert_eq!(reasons(&held, "blocking"), ["task-without-file"], "{held}");
    }

    /// A tarefa que mexe em código e não nomeia arquivo trava o plano, e a
    /// recusa traz, na mesma resposta, os arquivos que o mapa sugere para ela.
    /// A que declara no texto que não mexe em arquivo passa sem o campo.
    #[test]
    fn a_code_task_without_a_file_blocks_the_plan_and_comes_back_with_what_the_map_suggests() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Uma.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Somar dois números.", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Não mexe em arquivo de código: é escrita na spec.", "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        let blocking: Vec<&Value> = report["blocking"].as_array().map(Vec::as_slice).unwrap_or_default().iter().collect();
        let refused: Vec<&Value> = blocking.iter().copied().filter(|f| f["reason"] == json!("task-without-file")).collect();
        assert_eq!(refused.len(), 1, "só a tarefa de código é recusada: {report}");
        let hint = refused[0]["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("MSTD-TASK-0001"), "{hint}");
        assert!(hint.contains(translate("plan.no_suggestion", Locale::PtBr)), "o mapa vazio se anuncia: {hint}");
    }

    /// Cada achado da conferência é gravado como anotação quando o `plan`
    /// roda, com a mesma mensagem que a resposta dá e com o rótulo do achado
    /// do plano. Rodar de novo não repete a anotação.
    #[test]
    fn every_finding_is_written_as_a_note_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        std::fs::write(root.join("src/fora.rs"), "fn tres() {}\n").unwrap();
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "contract", json!({"text": "A barra tem duas linhas.", "example": "dev · x", "keys": ["barra"], "waves": [1], "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Mexer no arquivo e na spec.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer.", "files": [{"path": "src/fora.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Não mexe em arquivo: é escrita na spec.", "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "{report}");
        let messages: Vec<String> = report["warnings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|f| f["hint"].as_str().map(str::to_string))
            .collect();
        assert!(messages.len() >= 2, "{report}");

        // Cada achado virou anotação, com o texto exato.
        let notes = read_notes(root, "x");
        for message in &messages {
            assert!(notes.contains(message), "sem anotação de {message:?}: {notes:?}");
        }
        // Toda anotação de achado leva o rótulo, que é por onde a página as
        // recolhe.
        let label = translate("plan.finding.label", Locale::PtBr);
        assert!(!note_labels(root, "x").is_empty(), "nenhuma anotação de achado");
        assert!(note_labels(root, "x").iter().all(|l| l == label), "{:?}", note_labels(root, "x"));

        // De novo: as mesmas anotações, sem repetir nenhuma.
        assert_eq!(plan(root, "x")["ok"], json!(true));
        assert_eq!(read_notes(root, "x"), notes, "o mesmo achado não vira anotação duas vezes");
    }

    /// O achado que trava a pergunta também vira anotação: é nela que quem
    /// for corrigir o vê. O plano travado não é marco e não escreve página
    /// nem prepara cópia.
    #[test]
    fn a_blocking_finding_is_noted_and_writes_no_page() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Uma.", "criteria": [crit],
            "done_when": "passa", "depends_on": [7], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer.", "files": [{"path": "src/a.rs"}], "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        assert!(reasons(&report, "blocking").contains(&"depends-on-missing-wave".to_string()), "{report}");
        let hint = report["blocking"][0]["hint"].as_str().unwrap().to_string();
        assert!(read_notes(root, "x").contains(&hint), "o achado que trava não virou anotação");
        assert!(report.get("copy").is_none() && report.get("publish").is_none(), "{report}");
        assert!(!root.join(".claude/spec/x/spec.html").exists(), "o plano travado não escreve página");
        assert!(!root.join(".claude/spec/x/copy").exists(), "nem prepara cópia");
    }

    /// O rótulo de cada anotação vigente que tem um, em ordem de número.
    fn note_labels(root: &Path, spec: &str) -> Vec<String> {
        let log = store::read(&store::spec_file(root, spec).unwrap()).unwrap().unwrap();
        log.block(BlockQuery::Block(Block::Notes))
            .into_iter()
            .filter(|event| event.event_type == "note")
            .filter_map(|event| event.str_field("label").map(str::to_string))
            .collect()
    }

    /// As anotações vigentes da spec, em ordem de número.
    fn read_notes(root: &Path, spec: &str) -> Vec<String> {
        let log = store::read(&store::spec_file(root, spec).unwrap()).unwrap().unwrap();
        log.block(BlockQuery::Block(Block::Notes))
            .into_iter()
            .filter(|event| event.event_type == "note")
            .filter_map(|event| event.str_field("text").map(str::to_string))
            .collect()
    }

    /// Com um item de texto que parece senha, o plano manda publicar, copiar
    /// e fazer a pergunta de aprovação assim mesmo: o item fica fora da cópia,
    /// e a resposta diz o código dele, para ser expurgado. Expurgado o item, o
    /// aviso some e ele vai na cópia, com o trecho oculto.
    #[test]
    fn a_withheld_item_no_longer_holds_the_publish_and_is_named_until_it_is_purged() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);
        let note = write(root, Some("x"), "note",
            json!({"text": "A senha do banco: S3nh4F0rte", "keys": ["banco"], "origin": said}));
        let code = note["code"].as_str().unwrap_or_default().to_string();

        let held = plan(root, "x");
        assert_eq!(held["ok"], json!(true), "{held}");
        assert_eq!(held["publish"], json!(["spec", "project"]), "the publish is not held: {held}");
        assert_eq!(held["withheld"], json!([code]), "{held}");
        let next = held["next"].as_str().unwrap_or_default();
        assert!(next.contains(&code) && next.contains("write purge"), "{next}");
        let ask = translate("plan.next", Locale::PtBr)
            .replace("{question}", translate("approval.question", Locale::PtBr))
            .replace("{option}", translate("approval.option", Locale::PtBr));
        assert!(next.contains("write publish") && next.ends_with(&ask), "{next}");
        let warned = held["warnings"].as_array().cloned().unwrap_or_default();
        assert!(warned.iter().any(|w| w["reason"] == json!("page-check")
            && w["hint"].as_str().unwrap_or_default().contains(&code)), "{held}");
        let note_id = note["id"].as_u64().unwrap_or_default();
        assert!(!sent_items(root, &held).contains(&note_id), "the item stays out of the copy");
        let folder = std::fs::read_dir(root.join(".claude/spec/x/copy/ranges")).unwrap();
        for file in folder.flatten() {
            let body = std::fs::read_to_string(file.path()).unwrap();
            assert!(!body.contains("S3nh4F0rte"), "{}", file.path().display());
        }

        let purged = write(root, Some("x"), "purge", json!({"targets": [code], "reason": "secret"}));
        assert_eq!(purged["ok"], json!(true), "{purged}");
        let free = plan(root, "x");
        assert_eq!(free["publish"], json!(["spec", "project"]), "{free}");
        assert!(free.get("withheld").is_none(), "{free}");
        let writes = sent(root, &free, "spec");
        let range = writes
            .iter()
            .find(|w| w["body"]["items"].as_array().is_some_and(|items| items.iter().any(|i| i["id"].as_u64() == Some(note_id))))
            .expect("the range with the purged item goes again");
        let copied = range["body"]["items"].as_array().unwrap().iter().find(|i| i["id"].as_u64() == Some(note_id)).unwrap();
        assert_eq!(copied["text"], json!("A senha do banco: …"), "the purged item goes, with the excerpt hidden");
    }

    /// A dica de aprovar do plano lê a opção de aprovar do catálogo, a mesma
    /// que a testemunha da aprovação reconhece, nos dois idiomas.
    #[test]
    fn the_approval_hint_reads_the_option_from_the_catalog() {
        for (lang, config) in [
            (Locale::PtBr, "{}".to_string()),
            (Locale::EnUs, r#"{"language":{"text":"en-US"}}"#.to_string()),
        ] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let said = surveyed(root, "x");
            std::fs::write(root.join("mustard.json"), config.as_bytes()).unwrap();
            sound_plan(root, "x", said);

            let report = plan(root, "x");
            assert_eq!(report["ok"], json!(true), "{lang:?}: {report}");
            let next = report["next"].as_str().unwrap_or_default();
            let option = translate("approval.option", lang);
            assert!(next.contains(&format!("\"{option}\"")), "{lang:?}: {next}");
            assert!(!next.contains("{option}"), "{lang:?}: {next}");
        }
    }

    /// Uma tarefa da onda `wave`, num arquivo novo só dela.
    fn rated(root: &Path, said: u64, wave: u64, file: &str) -> Value {
        let body = json!({"wave": wave, "text": "Mexer no código.", "files": [{"path": file, "new": true}], "origin": said});
        write(root, Some("x"), "task", body)
    }

    /// As ondas `1..=count`, todas com o mesmo texto das tarefas.
    fn waves_of_code(root: &Path, said: u64, count: u64) {
        let crit = criterion(root, "x", said);
        for n in 1..=count {
            write(root, Some("x"), "wave", json!({"n": n, "text": "Mexer no código.", "criteria": [crit],
                "done_when": "passa", "origin": said}));
        }
    }

    /// A tarefa sem nota não segura a aprovação, não soma nada e o texto da
    /// pergunta não cita a nota do Scrum: o plano segue só com o que o
    /// catálogo já dizia antes de a nota existir.
    #[test]
    fn o_plano_nao_pede_nem_soma_nota() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        waves_of_code(root, said, 1);
        rated(root, said, 1, "src/um.rs");
        rated(root, said, 1, "src/dois.rs");

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "a tarefa sem nota não trava o plano: {report}");
        assert!(hints_of(&report, "blocking", "task-without-points").is_empty(), "{report}");
        assert!(hints_of(&report, "warnings", "wave-points-over-cap").is_empty(), "{report}");
        let next = report["next"].as_str().unwrap_or_default();
        let plain = translate("plan.next", Locale::PtBr)
            .replace("{question}", translate("approval.question", Locale::PtBr))
            .replace("{option}", translate("approval.option", Locale::PtBr));
        assert!(next.ends_with(&plain), "a pergunta não leva prefixo de nota: {report}");
        assert!(!next.contains("ponto"), "nenhum texto impresso cita a nota do Scrum: {next}");
    }

    /// A aprovação é o primeiro marco de uma spec nova, que ainda não tem
    /// template: a ordem manda publicar o template, sem falar de página
    /// antiga, e entregar a primeira cópia, a spec inteira, a um agente
    /// separado, antes da pergunta. Uma spec que uma versão antiga publicou
    /// inteira, ainda no plano, ganha o template num link novo na aprovação.
    #[test]
    fn the_first_copy_goes_to_an_agent_at_the_approval_of_a_new_spec() {
        use crate::commands::spec_events::pages::copy::{agent_order, batches_order, old_page_order};
        let lang = Locale::PtBr;
        for old in [false, true] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let said = surveyed(root, "x");
            waves_of_code(root, said, 1);
            rated(root, said, 1, "src/a1.rs");
            rated(root, said, 1, "src/b1.rs");
            rated(root, said, 1, "src/c1.rs");
            if old {
                let page = json!({"page": "spec", "milestone": "approval", "ok": true,
                    "url": "https://claude.ai/code/artifact/pagina-antiga"});
                assert_eq!(write(root, Some("x"), "publish", page)["ok"], json!(true));
            }

            let report = plan(root, "x");
            assert_eq!(report["ok"], json!(true), "{old}: {report}");
            let next = report["next"].as_str().unwrap_or_default();
            assert_eq!(report["publish"], json!(["spec", "project"]), "{old}: {report}");
            // A publicação grava o carimbo do molde que o programa monta.
            let stamp = mustard_core::platform::page_templates::spec_page_template(lang);
            let stamp = mustard_core::platform::page_templates::template_stamp(&stamp).expect("the stamp");
            let record = format!(
                r#"'{{"page":"spec","milestone":"approval","ok":true,"template":true,"stamp":"{stamp}","url":"…"}}'"#
            );
            assert!(next.contains(&record), "{old}: {next}");
            assert_eq!(next.contains(&old_page_order("spec", lang)), old, "{old}: {next}");
            assert_eq!(report["copy"]["spec"]["first"], json!(true), "{old}: {report}");
            assert_eq!(sent_items(root, &report).first(), Some(&1), "{old}: the whole spec");
            let copy = batches_order(&report, "x", translate("page.copy.new_address", lang), lang);
            let agent = agent_order(&copy, lang);
            assert!(next.contains(&agent), "{old}: the first copy goes to an agent: {next}");
            let ask = translate("plan.next", lang)
                .replace("{question}", translate("approval.question", lang))
                .replace("{option}", translate("approval.option", lang));
            assert!(next.find(&agent) < next.find(&ask) && next.ends_with(&ask), "{old}: {next}");
            assert!(report.get("migration").is_none(), "{old}: a migração de nota não existe mais: {report}");
        }
    }
}
