//! `mustard-rt run plan [--spec <nome>]` — o passo do plano: monta o pedido
//! de cada onda, confere tudo e leva a spec do levantamento para o plano.
//!
//! É a porta entre o levantamento e a aprovação. Depois que a especificação,
//! as ondas e as tarefas estão gravadas, este comando monta o pedido de cada
//! onda a partir dos eventos — com as lições e as skills —, confere o plano
//! contra o código real, refaz o índice, grava a fase `plan` pela mesma porta
//! de gravação de fase das outras e responde o próximo passo: conferir o
//! plano e fazer a pergunta de aprovação. Publicar a página é só a pedido do
//! usuário, e este passo não publica nem copia nada. O item que ainda guarda
//! um trecho com cara de segredo sai dito, para ser expurgado.
//!
//! **O que trava** e segura a pergunta até ser corrigido: ponto do
//! levantamento aberto; skill que a conferência recusa; arquivo citado que
//! não existe e que nenhum item da spec marca como novo; critério cuja prova não é um
//! comando de verdade (programa que não roda, comandos ligados por `;`, busca
//! sozinha que devia sair vazia); e tarefa que mexe em código sem dizer em
//! que arquivo, que volta pedindo para nomeá-lo: o plano nunca adivinha o
//! arquivo pelo texto da tarefa.
//!
//! O item combinado sem dono — nenhuma tarefa de uma onda do plano o cobre,
//! ele não diz as ondas dele nem vale no projeto todo — não trava nem avisa:
//! a análise antes do envio de cada onda decide se ele vai para ela.
//!
//! **O que só avisa**, e a decisão fica com quem aprova: arquivo citado fora
//! do git (um agente noutra sessão ou máquina não o vê); nome citado que o
//! mapa não acha; item combinado de uma onda que nenhuma tarefa cobre — menos
//! o marcado como "não vira código", que traz o motivo na linha dele, e o do
//! projeto, que vale sempre; contrato que nenhum critério cita; e o comando
//! de compilar ou de testar que o `mustard.json` ainda não declara, com o
//! campo a preencher. A skill não se sugere por palavras: a que declara as
//! pastas que atende entra no pedido da onda que mexe nelas.
//!
//! O plano não confere onda: a onda é um lote que o binário forma na hora de
//! despachar, e não um desenho do plano. Não há aviso de ondas que dividem
//! arquivo, de onda ou spec que podia sair dividida, nem de tarefa cujo texto
//! não casa com o da onda; a trava de dois lotes em paralelo nunca dividirem
//! arquivo mora na rodada.
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
use mustard_core::domain::wave_times::{WaveTimes, wave_times};
use mustard_core::io::citation::DiskWorld;
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{Flight, WavePrompt, prompts};
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use crate::commands::spec_events::{self, write::record};
use crate::shared::spec_state::{DiskSpecState, checkout, session_from_env};

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
    /// Uma recusa do arquivo de eventos: o ponto do levantamento que ficou
    /// aberto.
    Refused(Refusal),
    /// Uma skill que a conferência recusou.
    Skill { name: String, refusal: MapRefusal },
    /// Um arquivo que a tarefa cita e que o git não guarda.
    FileOutsideGit { task: String, path: String },
    /// Um nome citado que o mapa do projeto não confirma.
    Cited { task: String, finding: Finding },
    /// Um item combinado que nenhuma tarefa cobre.
    ItemWithoutTask { code: String },
    /// Um contrato que nenhum critério cita.
    ContractWithoutCriterion { code: String },
    /// Uma tarefa que mexe em código e não diz em que arquivo mexe.
    TaskWithoutFile { task: String },
    /// O comando de compilar ou de testar que o projeto ainda não declarou
    /// no `mustard.json`, pelo nome do campo (`buildCommand`/`testCommand`).
    CommandNotDeclared { field: &'static str },
    /// Um critério cuja prova não é um comando que se sustente: a rodada e o
    /// fechamento a rodariam e ela passaria ou falharia sem provar nada.
    Proof { criterion: String, refusal: Refusal },
    /// Uma tarefa que espera outra, dividindo com ela poucos arquivos: mover
    /// esses arquivos para uma das duas tira a dependência, e as ondas das
    /// duas passam a sair juntas.
    DependencyOnlyFiles { task: String, waits_for: String, files: Vec<String> },
}

impl PlanFinding {
    /// `true` para o achado que segura a pergunta de aprovação.
    fn blocks(&self) -> bool {
        match self {
            Self::Refused(_) | Self::Skill { .. } | Self::TaskWithoutFile { .. } | Self::Proof { .. } => true,
            Self::Cited { finding, .. } => finding.is_refusal(),
            Self::FileOutsideGit { .. }
            | Self::ItemWithoutTask { .. }
            | Self::ContractWithoutCriterion { .. }
            | Self::CommandNotDeclared { .. }
            | Self::DependencyOnlyFiles { .. } => false,
        }
    }

    /// A razão curta, estável, para quem lê a saída por máquina.
    fn reason(&self) -> String {
        match self {
            Self::Refused(refusal) | Self::Proof { refusal, .. } => refusal.reason().to_string(),
            Self::Skill { refusal, .. } => refusal.reason().to_string(),
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
            Self::CommandNotDeclared { .. } => "command-not-declared".into(),
            Self::DependencyOnlyFiles { .. } => "dependency-only-files".into(),
        }
    }

    /// A mensagem exata, no idioma pedido.
    fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value));
        match self {
            Self::Refused(refusal) => refusal.message(lang),
            Self::Skill { name, refusal } => {
                format!("{name}: {}", refusal.message(lang))
            }
            Self::Proof { criterion, refusal } => format!("{criterion}: {}", refusal.message(lang)),
            Self::FileOutsideGit { task, path } => fill("plan.file_outside_git", &[("{task}", task.clone()), ("{path}", path.clone())]),
            // A conferência das citações é a mesma do ponto do levantamento, e
            // as mensagens dela também: o número do fato vira o código da
            // tarefa que citou.
            Self::Cited { task, finding } => {
                let text = finding.refusal(0).map(|refusal| refusal.message(lang)).or_else(|| finding.warning(0, lang)).unwrap_or_default();
                format!("{task}: {text}")
            }
            Self::ItemWithoutTask { code } => fill("plan.item_without_task", &[("{code}", code.clone())]),
            Self::ContractWithoutCriterion { code } => fill("plan.contract_without_criterion", &[("{code}", code.clone())]),
            Self::TaskWithoutFile { task } => fill("plan.task_without_file", &[("{task}", task.clone())]),
            Self::CommandNotDeclared { field } => fill("plan.command_not_declared", &[("{field}", (*field).to_string())]),
            Self::DependencyOnlyFiles { task, waits_for, files } => {
                let shown: Vec<String> = files.iter().map(|path| format!("`{path}`")).collect();
                let key = if files.len() == 1 { "plan.dependency_only_files.one" } else { "plan.dependency_only_files.many" };
                fill(key, &[("{task}", task.clone()), ("{waits_for}", waits_for.clone()), ("{files}", shown.join(", "))])
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
    if let Err(refusal) = mustard_core::io::legacy_specs::refuse_legacy(&project.root, &spec) {
        return refuse(&refusal);
    }
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
    let host=crate::shared::host_observation::caller_host(&project.root,crate::shared::spec_state::session_from_env().as_deref());
    let built = prompts(&project.root, &spec, &log, lang, &Flight { host:Some(host.into()), running, ..Flight::default() }, &log.planned_waves());
    let findings = check(&opts.root, &project.root, &spec, &log, &built);
    // Cada achado vira anotação no arquivo de eventos, aqui, uma vez por
    // plano: a página o mostra de lá, sem olhar o disco nem o git ao ser
    // desenhada. As duas conferências que olham para fora do arquivo — o
    // arquivo citado existe, o arquivo está no git — ficam nesta chamada.
    if let Err(refusal) = note_findings(&opts.root, &spec, &log, &findings, lang) {
        return refuse(&refusal);
    }
    let blocking: Vec<Value> = findings.iter().filter(|f| f.blocks()).map(|f| f.to_value(lang)).collect();
    let warnings: Vec<Value> = findings.iter().filter(|f| !f.blocks()).map(|f| f.to_value(lang)).collect();
    let waves: Vec<Value> = built.iter().map(|p| json!({ "wave": p.wave, "lines": p.lines, "skills": p.stale_skills })).collect();
    let tasks = task_sizes(&log);

    if !blocking.is_empty() {
        // Com a spec já em plano, o plano recusado deixa a marca dele: um
        // `state` na mesma fase, com a razão da recusa. A aprovação lê o
        // último plano por ela e fica travada até um plano passar de novo,
        // mesmo quando a recusa veio só do disco, sem nada novo gravado.
        if let Err(refusal) = mark_refused_plan(&opts.root, &spec, &log) {
            return refuse(&refusal);
        }
        // O plano travado não é marco: nada vai para o banco da página, e o
        // item que guarda um trecho com cara de segredo sai dito, para ser
        // expurgado antes da cópia.
        let mut report = json!({
            "ok": false, "spec": spec, "reason": "plan-not-ready",
            "hint": translate("plan.not_ready", lang).replace("{count}", &blocking.len().to_string()),
            "waves": waves, "tasks": tasks, "blocking": blocking, "warnings": warnings,
        });
        let withheld = spec_events::pages::secrets::withheld(&log);
        spec_events::pages::note_withheld(&mut report, &spec, &withheld, lang);
        return report;
    }

    // A formação das ondas sai antes da aprovação, pela mesma leitura que a
    // rodada pede: com o pedido ao agente de análise ainda sem recibo, o
    // plano para aqui, sem gravar a fase nem fazer a pergunta, e roda de novo
    // depois do recibo. Com a formação gravada, a spec é relida.
    let forming = crate::commands::flow::triage::form_for_plan(&opts.root, &spec, session);
    if let Some(pending) = forming.pending {
        return json!({
            "ok": true, "spec": spec, "waves": waves, "tasks": tasks, "warnings": warnings,
            "formation": pending["grouping"], "next": translate("plan.formation_pending", lang),
        });
    }
    let log = if forming.applied {
        match store::read(&path) {
            Ok(Some(log)) => log,
            Ok(None) => return refuse(&Refusal::NoSpecFile { spec }),
            Err(refusal) => return refuse(&refusal),
        }
    } else {
        log
    };
    let tasks = task_sizes(&log);

    // A fase passa pela mesma porta de gravação de fase das outras, e ela já
    // refaz a linha da spec no índice. Uma spec que já está em plano, com o
    // último plano passado e nada do plano gravado depois dele, não grava
    // nada e tem a linha refeita aqui; com o plano mudado ou recusado desde
    // então, grava a marca nova do plano que passou, que é o que a aprovação
    // lê.
    let from = State::from_log(&log).phase.unwrap_or("-").to_string();
    let recorded = if from == "plan" && mustard_core::domain::spec_state::plan_passed(&log) {
        if let Ok(paths) = mustard_core::ClaudePaths::for_project(&project.root)
            && let Err(refusal) = mustard_core::io::spec_index::refresh_line(&paths.spec_index_path(), &spec, &log)
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

    // O passo termina na pergunta de aprovação, sem publicar nem copiar nada:
    // publicar a página é só a pedido do usuário, e o acompanhamento mora no
    // painel. O item que ainda guarda um trecho com cara de segredo sai dito,
    // para ser expurgado, sem segurar a pergunta.
    let mut report = json!({
        "ok": true, "spec": spec, "phase": "plan", "from": from,
        "waves": waves, "tasks": tasks, "warnings": warnings,
    });
    if let Some(id) = recorded {
        report["id"] = json!(id);
    }
    // O bloco que vai antes da pergunta: as ondas e as etapas pela formação,
    // e o tempo das ondas pela história das specs do próprio projeto.
    let specs = mustard_core::io::spec_index::read_specs(&project.root);
    report["forecast"] = forecast(&log, wave_times(specs.iter().map(|(_, log)| log)), lang);
    // A pergunta vai com o texto exato do catálogo: a testemunha da aprovação
    // só reconhece essa pergunta, e outro texto não aprova nada.
    let ask = translate("plan.next", lang).replace("{question}", translate("approval.question", lang)).replace("{option}", translate("approval.option", lang));
    report["next"] = json!(ask);
    report["panel"] = json!("/mustard:mods");
    let withheld = spec_events::pages::secrets::withheld(&log);
    spec_events::pages::note_withheld(&mut report, &spec, &withheld, lang);
    report
}

/// A marca do plano recusado: um `state` em `plan` com a razão
/// [`PLAN_NOT_READY`](mustard_core::domain::spec_state::PLAN_NOT_READY), só
/// só com a spec em plano e só quando a última marca não é já a de uma
/// recusa. Fora do plano a aprovação já está travada, e a marca repetida não
/// diria nada de novo.
///
/// # Errors
///
/// A recusa da gravação do estado.
fn mark_refused_plan(start: &Path, spec: &str, log: &SpecLog) -> Result<(), Refusal> {
    use mustard_core::domain::spec_state::{PLAN_NOT_READY, plan_refused};
    if State::from_log(log).phase != Some("plan") || plan_refused(log) {
        return Ok(());
    }
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("plan"));
    draft.insert("reason".to_string(), json!(PLAN_NOT_READY));
    draft.insert("author".to_string(), json!("binary"));
    record(start, spec, "state", draft, PhaseWriter::Binary).map(|_| ())
}

/// Grava cada achado da conferência como anotação, no idioma do projeto, que
/// é o da página. Cada uma leva o rótulo do achado do plano, que é por onde a
/// página as recolhe na seção própria delas. O achado que já está anotado não
/// é gravado de novo: rodar o `plan` outra vez não repete a anotação.
///
/// # Errors
///
/// A recusa da gravação da anotação.
fn note_findings(start: &Path, spec: &str, log: &SpecLog, findings: &[PlanFinding], lang: Locale) -> Result<(), Refusal> {
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
fn check(start: &Path, root: &Path, spec: &str, log: &SpecLog, built: &[WavePrompt]) -> Vec<PlanFinding> {
    let mut out: Vec<PlanFinding> = Vec::new();
    let codes = log.codes();
    let code_of = |event: &SpecEvent| codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());

    // Nenhum ponto do levantamento aberto.
    let open = open_points(log);
    if !open.is_empty() {
        out.push(PlanFinding::Refused(open_refusal(spec, log, &open)));
    }

    // Um plano sem tarefa nenhuma não tem o que aprovar.
    if !log.visible().iter().any(|event| event.event_type == "task") {
        out.push(PlanFinding::Refused(Refusal::PlanWithoutTask { spec: spec.to_string() }));
    }

    // A onda já entregue: as conferências das tarefas olham só o que ainda
    // vem.
    let delivered = log.delivered_waves();

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
    // os nomes citados existem no mapa. O arquivo citado só pelo nome, que
    // não existe, sai com os arquivos do projeto que têm esse nome.
    let roots = store::citation_roots(start, root);
    let world = DiskWorld::new(roots, Some(root));
    let tasks: Vec<&SpecEvent> = tasks_of(log);
    // A conferência olha só o que ainda vem: a tarefa de onda que já tem
    // registro de entrega está provada pelo código que entrou, pelo commit
    // que a carrega e pela revisão que a aprovou, e conferir de novo o texto
    // que a descreveu só acumula trava que ninguém vai consertar.
    let ahead: Vec<&SpecEvent> = tasks.iter().copied().filter(|task| !task.wave().is_some_and(|n| delivered.contains(&n))).collect();
    // O arquivo que algum item da spec declara criar conta como existente,
    // gravado antes ou depois de quem o cita: a tarefa que espera outra cita
    // o arquivo que a outra cria, e o critério, gravado antes das tarefas que
    // o cobrem, cita o teste que uma delas vai escrever.
    let created = spec_events::proof_check::spec_created_files(log);
    let by_another = |path: &str| created.contains(&spec_events::proof_check::relative_path(path));

    let mut cited: Vec<String> = Vec::new();
    for task in &ahead {
        let code = code_of(task);
        let files = declared_files(task);
        let text = task.str_field("text").unwrap_or_default();
        if files.is_empty() && !says_it_touches_no_file(text) {
            out.push(PlanFinding::TaskWithoutFile { task: code.clone() });
        }
        for (path, new) in &files {
            if !new && !by_another(path) && world.file_lines(path).is_none() {
                out.push(PlanFinding::Cited { task: code.clone(), finding: citation::missing_file(&world, path) });
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
    let tracked: BTreeSet<String> = mustard_core::platform::git_exclude::tracked_paths(root, &cited).into_iter().collect();
    for task in &ahead {
        let code = code_of(task);
        for (path, new) in declared_files(task) {
            if !new && !by_another(&path) && !tracked.contains(&path) {
                out.push(PlanFinding::FileOutsideGit { task: code.clone(), path });
            }
        }
    }

    // A tarefa que espera outra dividindo com ela poucos arquivos: mover esses
    // arquivos para uma das duas tira a dependência, e as duas ondas passam a
    // sair juntas. Na correção do usuário, a segunda tarefa esperou a primeira
    // só porque as duas mexiam num mesmo arquivo de teste, e nada na resposta
    // dizia que a espera tinha saída.
    for task in &ahead {
        let mine: BTreeSet<String> = declared_files(task).into_iter().map(|(path, _)| path).collect();
        for waited in task.ints("depends_on").into_iter().filter_map(|id| log.current(id)).filter(|e| e.event_type == "task") {
            let shared: Vec<String> = declared_files(waited).into_iter().map(|(path, _)| path).filter(|path| mine.contains(path)).collect();
            if !shared.is_empty() && shared.len() <= MOVABLE_OVERLAP {
                out.push(PlanFinding::DependencyOnlyFiles { task: code_of(task), waits_for: code_of(waited), files: shared });
            }
        }
    }

    // A cobertura: o item de uma onda sem tarefa e o contrato sem critério só
    // avisam, e a decisão fica com quem aprova. O item sem dono não é
    // conferido aqui: a análise antes do envio de cada onda decide se ele vai
    // para ela.
    let covered: BTreeSet<u64> = tasks.iter().flat_map(|task| task.ints("covers")).filter_map(|id| log.current(id).map(|e| e.id)).collect();
    let agreed: Vec<&SpecEvent> =
        log.block(BlockQuery::Block(Block::Agreed)).into_iter().filter(|e| e.str_field("text").is_some_and(|t| !t.trim().is_empty())).collect();
    // O item do projeto é regra que vale sempre, sem tarefa que o implemente;
    // o marcado como "não vira código" traz o motivo na própria linha. Avisar
    // sobre eles seria avisar para sempre.
    let owners = wave_prompt::owners(log);
    for item in &agreed {
        match owners.get(&item.id) {
            None | Some(Owner::EveryWave | Owner::Project | Owner::Files(_)) => {}
            Some(Owner::Waves(_)) => {
                if !covered.contains(&item.id) && item.str_field("no_code").is_none() {
                    out.push(PlanFinding::ItemWithoutTask { code: code_of(item) });
                }
            }
        }
    }
    // A prova de cada critério é o comando que a rodada e o fechamento
    // rodam: a que não roda, liga comandos por `;`, é uma busca sem `!` ou
    // cita um alvo de teste ou um caminho que o projeto não tem, nem um item
    // da spec declara criar, segura a pergunta de aprovação. É aqui que a
    // existência do que o critério cita é conferida com todos os itens
    // gravados: antes da aprovação, a gravação do critério a deixa para cá.
    for criterion in log.block(BlockQuery::Block(Block::Criteria)).into_iter().filter(|e| e.event_type == "criterion") {
        let refusal = criterion.str_field("proof").and_then(|proof| spec_events::proof_check::proof_defect(root, proof, &created));
        if let Some(refusal) = refusal {
            out.push(PlanFinding::Proof { criterion: code_of(criterion), refusal });
        }
    }

    let with_criterion: BTreeSet<u64> =
        log.block(BlockQuery::Block(Block::Criteria)).into_iter().flat_map(|e| e.ints("contracts")).filter_map(|id| log.current(id).map(|e| e.id)).collect();
    for contract in agreed.iter().filter(|e| e.event_type == "contract") {
        if !with_criterion.contains(&contract.id) {
            out.push(PlanFinding::ContractWithoutCriterion { code: code_of(contract) });
        }
    }
    out
}

/// Quantos arquivos em comum ainda cabem numa mudança pequena: até dois,
/// mover os arquivos de uma tarefa para a outra é um ajuste; acima disso, o
/// que está em jogo é a divisão do plano, e sugerir mover arquivo nenhum
/// ajudaria.
const MOVABLE_OVERLAP: usize = 2;

/// As tarefas do plano, na ordem do arquivo de eventos. O dono único da
/// pergunta "quais são as tarefas desta spec": a conferência as olha para
/// achar defeito, e a resposta do plano as conta.
fn tasks_of(log: &SpecLog) -> Vec<&SpecEvent> {
    log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|event| event.event_type == "task").collect()
}

/// Cada tarefa do plano com a onda dela e a contagem de arquivos que ela
/// declara.
///
/// A contagem vai sempre, sem aviso, sem teto e sem configuração. Uma tarefa
/// de vinte e nove arquivos e uma de um saíam iguais na resposta, e quem
/// aprovava não tinha como ver o tamanho do que ia aprovar. Um aviso com
/// número fixo seria o teto que o usuário tirou: a contagem basta, e a
/// decisão é de quem lê.
fn task_sizes(log: &SpecLog) -> Vec<Value> {
    let codes = log.codes();
    tasks_of(log)
        .into_iter()
        .map(|task| {
            let code = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
            let mut row = json!({"task": code, "files": declared_files(task).len()});
            if let Some(wave) = task.wave() {
                row["wave"] = json!(wave);
            }
            row
        })
        .collect()
}

/// Onde a rodada põe uma tarefa: na onda que ela já tem, no assunto que a
/// formação deu, ou sozinha, que a tarefa sem assunto já é a própria onda.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    Wave(u64),
    Subject(String),
    Alone(u64),
}

/// As ondas que as tarefas ainda por entregar de `log` vão formar e as
/// etapas, a cadeia mais longa de ondas em que cada uma espera uma tarefa da
/// anterior.
struct WaveCount {
    waves: usize,
    stages: usize,
    /// Toda tarefa já tem onda ou assunto da formação. Com uma tarefa sem
    /// nenhum dos dois, ela conta como uma onda sozinha, e o número de ondas
    /// é um teto: a formação ainda pode juntá-la a outras.
    formed: bool,
}

/// O [`WaveCount`] de `log`, pela formação gravada nas tarefas.
fn waves_and_stages(log: &SpecLog) -> WaveCount {
    use crate::commands::flow::round::queue::{backlog_population, tasks_not_delivered, waves_returned_without_replan};
    use std::collections::BTreeMap;
    let returned = waves_returned_without_replan(log);
    let left: BTreeSet<u64> =
        tasks_not_delivered(log, &BTreeSet::new()).into_iter().filter(|id| log.get(*id).and_then(SpecEvent::wave).is_none_or(|w| !returned.contains(&w))).collect();
    let slot_of = |task: &SpecEvent| {
        let subject = task.fields.get("group").filter(|group| group.get("formed") == Some(&json!(true))).and_then(|group| group.get("name")?.as_str());
        match (task.wave(), subject) {
            (Some(wave), _) => Slot::Wave(wave),
            (None, Some(name)) => Slot::Subject(name.to_string()),
            (None, None) => Slot::Alone(task.id),
        }
    };
    let slots: BTreeMap<u64, Slot> = left.iter().filter_map(|id| log.get(*id).map(|task| (*id, slot_of(task)))).collect();
    let mut waits: BTreeMap<&Slot, BTreeSet<&Slot>> = slots.values().map(|slot| (slot, BTreeSet::new())).collect();
    for task in backlog_population(log, &BTreeSet::new()) {
        let Some(mine) = slots.get(&task.id) else { continue };
        for other in task.depends_on.iter().filter_map(|id| slots.get(id)).filter(|other| *other != mine) {
            waits.entry(mine).or_default().insert(other);
        }
    }
    fn chain<'a>(slot: &'a Slot, waits: &BTreeMap<&'a Slot, BTreeSet<&'a Slot>>, known: &mut BTreeMap<&'a Slot, usize>, open: &mut BTreeSet<&'a Slot>) -> usize {
        if let Some(length) = known.get(slot) {
            return *length;
        }
        // Um círculo de esperas não se conta duas vezes: a conferência das
        // dependências o recusa noutro passo.
        if !open.insert(slot) {
            return 0;
        }
        let length = 1 + waits.get(slot).into_iter().flatten().map(|other| chain(other, waits, known, open)).max().unwrap_or(0);
        open.remove(slot);
        known.insert(slot, length);
        length
    }
    let mut known = BTreeMap::new();
    let stages = waits.keys().map(|slot| chain(slot, &waits, &mut known, &mut BTreeSet::new())).max().unwrap_or(0);
    WaveCount { waves: waits.len(), stages, formed: !slots.values().any(|slot| matches!(slot, Slot::Alone(_))) }
}

/// O bloco que o plano diz antes da pergunta de aprovação, no idioma `lang`:
/// as ondas e as etapas da obra de `log`, e a medida `times` das ondas do
/// projeto, com o total estimado pela onda do meio vezes as etapas. Sem a
/// formação de alguma tarefa, o número de ondas sai dito como teto, uma onda
/// por tarefa sem assunto. Sem onda medida, o texto diz que ainda não há
/// medida, sem número nenhum.
fn forecast(log: &SpecLog, times: Option<WaveTimes>, lang: Locale) -> Value {
    let WaveCount { waves, stages, formed } = waves_and_stages(log);
    let key = if formed { "plan.forecast.waves" } else { "plan.forecast.ceiling" };
    let mut text = translate(key, lang).replace("{waves}", &waves.to_string()).replace("{stages}", &stages.to_string());
    let mut block = json!({ "waves": waves, "stages": stages, "formed": formed, "wave_minutes": times });
    match times {
        Some(times) => {
            let estimate = times.median * stages as u64;
            block["estimate_minutes"] = json!(estimate);
            let said = translate("plan.forecast.times", lang)
                .replace("{measured}", &times.measured.to_string())
                .replace("{shortest}", &times.shortest.to_string())
                .replace("{median}", &times.median.to_string())
                .replace("{longest}", &times.longest.to_string())
                .replace("{estimate}", &estimate.to_string());
            text = format!("{text} {said}");
        }
        None => text = format!("{text} {}", translate("plan.forecast.no_measure", lang)),
    }
    block["text"] = json!(text);
    block
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
            (!path.is_empty()).then(|| (path, file.get("new").and_then(Value::as_bool) == Some(true)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::flow::grill::{GrillOpts, grill_for};
    use crate::commands::spec_events::write::{WriteOpts, record_open, seed_at};
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

    /// Um projeto com dois arquivos no git e uma spec cujo levantamento
    /// terminou: todos os pontos fechados, pronta para o plano.
    fn surveyed(root: &Path, spec: &str) -> u64 {
        let source = "fn one() {}\nfn dois() {}\n";
        crate::shared::test_fixture::seeded_repo(root, &[("mustard.json", "{}"), ("src/a.rs", source), ("src/b.rs", source)]);

        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, Some(spec), "message", json!({"author": "user", "text": GOAL})));
        id_of(&write(root, Some(spec), "context", json!({"text": GOAL, "origin": said})));
        let opts = GrillOpts { root: root.to_path_buf(), spec: Some(spec.into()), kinds: Some("fix".into()), condensed: false };
        // O `grill` grava os pontos abertos; cada um é fechado pelo número.
        let listed = grill_for(&opts, None);
        for item in listed["points"].as_array().cloned().unwrap_or_default() {
            let closing = json!({"block": item["block"], "gap": item["gap"], "from": "gap",
                "status": "not_applicable", "closes": id_of(&item),
                "reason": "Já respondido.", "origin": said});
            assert_eq!(write(root, Some(spec), "point", closing)["ok"], json!(true));
        }
        said
    }

    /// Um critério da spec, que toda onda precisa apontar.
    fn criterion(root: &Path, spec: &str, said: u64) -> u64 {
        id_of(&write(
            root,
            Some(spec),
            "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test", "form": "ubiquitous",
                "origin": said}),
        ))
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
        report[field].as_array().cloned().unwrap_or_default().iter().filter_map(|f| f["reason"].as_str().map(str::to_string)).collect()
    }

    /// Um plano são é conferido numa chamada: a spec passa para o plano, sem
    /// página, `.md` nem cópia escritos, e a resposta manda fazer a pergunta
    /// de aprovação, sem ordem de publicar ou de copiar.
    #[test]
    fn a_sound_plan_is_checked_in_one_call_and_asks_the_question() {
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
        assert!(report.get("publish").is_none() && report.get("copy").is_none(), "{report}");
        assert!(!next.contains("ArtifactData") && !next.contains("write publish"), "{next}");
        assert_eq!(report["panel"], "/mustard:mods");
        assert!(!root.join(".claude/mustard/publications").exists());
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
        assert!(again.get("publish").is_none() && again.get("copy").is_none(), "{again}");
        let rebuilt = std::fs::read_to_string(root.join(".claude/spec/index.ndjson")).unwrap();
        assert_eq!(rebuilt, index, "o índice volta igual");
    }

    /// Com a spec no plano, o próximo passo manda conferir o plano e fazer a
    /// pergunta de aprovação, e nunca publicar a página: publicação é só a
    /// pedido do usuário. Vale nos dois idiomas, na resposta do plano, na
    /// retomada, no bloco do começo da sessão e na rodada pedida antes do sim.
    #[test]
    fn the_plan_next_step_does_not_ask_to_publish() {
        use crate::commands::flow::resume::{ResumeOpts, resume_block, resume_for};
        use crate::commands::flow::round::{RoundOpts, round_for};
        use crate::hooks::session::session_start_inject::MAX_BYTES;

        let cases = [
            ("pt-BR", Locale::PtBr, "confira o plano", "Aprovar esta spec?"),
            ("en-US", Locale::EnUs, "check the plan", "Approve this spec?"),
        ];
        for (language, lang, check, question) in cases {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let said = surveyed(root, "x");
            sound_plan(root, "x", said);
            std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{language}"}}}}"#)).unwrap();

            let planned = plan(root, "x");
            assert_eq!(planned["phase"], json!("plan"), "{language}: {planned}");
            let resumed = resume_for(&ResumeOpts { root: root.to_path_buf(), spec: Some("x".into()) }, None);
            let next = resumed["next"].as_str().unwrap_or_default();
            assert!(next.contains(check) && next.contains(&format!("\"{question}\"")), "{language}: {next}");
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let block = resume_block("x", &log, &crate::commands::spec_events::conversation::observed(root, "x", &log), lang, MAX_BYTES);
            let round = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
            assert_eq!(round["reason"], json!("round-not-approved"), "{language}: {round}");

            let said = [planned["next"].to_string(), next.to_string(), block, round.to_string()];
            for text in said {
                let lower = text.to_lowercase();
                for word in ["publique", "publicar", "publish"] {
                    assert!(!lower.contains(word), "{language}: `{word}` in {text}");
                }
            }
        }
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
        write(
            root,
            Some("x"),
            "rule",
            json!({"text": "No máximo 3 tentativas de compilação.",
            "example": "a quarta para", "keys": ["tentativas"], "applies_to": {"files": ["**"]}, "origin": said}),
        );
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Escrever a soma.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Escrever a soma.", "files": [{"path": "src/a.rs"}], "origin": said}));
        write(
            root,
            Some("x"),
            "wave",
            json!({"n": 2, "text": "Escrever a subtração.", "criteria": [crit], "done_when": "passa", "depends_on": [1], "origin": said}),
        );
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Escrever a subtração.", "files": [{"path": "src/b.rs"}], "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "{report}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let built = prompts(root, "x", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        assert_eq!(built.len(), 2, "duas ondas, dois pedidos");
        assert!(report.get("copy").is_none() && report.get("publish").is_none());
        assert!(!root.join(".claude/spec/x/copy").exists());
        assert_eq!(crate::commands::panel::snapshot(root, Some("x"))["specs"][0]["waves"].as_array().unwrap().len(), 2);
        let returns = format!("## {}", translate("prompt.part.return", Locale::PtBr));
        assert!(built.iter().all(|prompt| prompt.text.contains(&returns)), "{built:?}");
    }

    /// A resposta do plano traz cada tarefa com a contagem de arquivos dela,
    /// sempre, e a tarefa que espera outra dividindo com ela um arquivo ganha
    /// a sugestão de mover o arquivo e tirar a dependência. É aviso, nunca
    /// recusa: quem aprova decide. Acima de dois arquivos em comum, mover não
    /// é ajuste pequeno, e o plano não sugere nada.
    #[test]
    fn the_plan_counts_each_task_files_and_offers_to_free_a_dependency_over_shared_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Escrever a soma.", "criteria": [crit], "done_when": "A suíte passa.", "origin": said}));
        let first = id_of(&write(root, Some("x"), "task", json!({"wave": 1, "text": "Escrever a soma.", "files": [{"path": "src/a.rs"}], "origin": said})));
        write(
            root,
            Some("x"),
            "wave",
            json!({"n": 2, "text": "Escrever a subtração.", "criteria": [crit], "done_when": "A suíte passa.", "depends_on": [1], "origin": said}),
        );
        let second = id_of(&write(
            root,
            Some("x"),
            "task",
            json!({"wave": 2, "text": "Escrever a subtração.", "files": [{"path": "src/a.rs"}, {"path": "src/b.rs"}],
            "depends_on": [first], "origin": said}),
        ));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "o plano segue: a sugestão é aviso: {report}");
        assert_eq!(
            report["tasks"],
            json!([{"task": "MSTD-TASK-0001", "wave": 1, "files": 1}, {"task": "MSTD-TASK-0002", "wave": 2, "files": 2}]),
            "{report}"
        );
        let suggested: Vec<&Value> =
            report["warnings"].as_array().into_iter().flatten().filter(|w| w["reason"] == json!("dependency-only-files")).collect();
        assert_eq!(suggested.len(), 1, "{report}");
        let hint = suggested[0]["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("MSTD-TASK-0002") && hint.contains("MSTD-TASK-0001"), "a sugestão nomeia as duas tarefas: {hint}");
        assert!(hint.contains("`src/a.rs`") && !hint.contains("src/b.rs"), "só o arquivo em comum: {hint}");

        // Com três arquivos em comum, mover deixa de ser ajuste: nada se
        // sugere, e a contagem segue saindo.
        crate::shared::test_fixture::seeded_repo(root, &[("src/c.rs", "fn tres() {}\n")]);
        let shared = json!([{"path": "src/a.rs"}, {"path": "src/b.rs"}, {"path": "src/c.rs"}]);
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Escrever a soma.", "files": shared, "replaces": first, "origin": said}));
        write(
            root,
            Some("x"),
            "task",
            json!({"wave": 2, "text": "Escrever a subtração.", "files": shared, "depends_on": [first], "replaces": second, "origin": said}),
        );
        let again = plan(root, "x");
        assert!(!reasons(&again, "warnings").contains(&"dependency-only-files".to_string()), "{again}");
        assert!(again["tasks"].as_array().unwrap().iter().all(|row| row["files"] == json!(3)), "{again}");
    }

    /// Um ponto do levantamento ainda aberto trava a pergunta de aprovação, e
    /// a spec não passa para o plano.
    #[test]
    fn an_open_survey_point_blocks_the_approval_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);
        write(
            root,
            Some("x"),
            "point",
            json!({"block": "limits", "gap": "o teto", "from": "gap",
            "status": "open", "facts": [{"text": GOAL, "source": "src/a.rs:1"}], "origin": said}),
        );

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        assert_eq!(report["reason"], json!("plan-not-ready"));
        assert!(reasons(&report, "blocking").contains(&"survey-open".to_string()), "{report}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(State::from_log(&log).phase, Some("survey"), "a fase não andou");
    }

    /// Uma linha crua, direto no arquivo da spec, sem passar pela gravação:
    /// simula uma onda que já existia no arquivo, de antes do backlog, ou uma
    /// edição feita fora do binário.
    fn append_raw(root: &Path, spec: &str, event_type: &str, body: Value, id: u64) {
        let mut map = mustard_core::domain::spec_events::normalize(body.as_object().cloned().unwrap_or_default(), event_type);
        map.insert("type".into(), json!(event_type));
        let line = mustard_core::domain::spec_events::render_line(&mustard_core::domain::spec_events::stamp(map, id, None, "2026-09-20T10:00:00-03:00"));
        use std::io::Write as _;
        let path = store::spec_file(root, spec).unwrap();
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(file, "{line}").unwrap();
    }

    /// Um pedido bem além do antigo teto de 500 linhas não trava mais a
    /// pergunta: o teto não existe. As 600 tarefas vêm direto no arquivo,
    /// sem passar pela gravação, e o plano confere a onda do arquivo do
    /// mesmo jeito que a gravada.
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
            append_raw(
                root,
                "x",
                "task",
                json!({"wave": 1, "text": "Somar.", "files": [{"path": "src/a.rs"}],
                "skill": format!("s{i}"), "origin": said}),
                first_id + i,
            );
        }

        let report = plan(root, "x");
        assert!(!reasons(&report, "blocking").contains(&"wave-prompt-too-long".to_string()), "{report}");
        assert!(report["waves"][0]["lines"].as_u64().unwrap() > 500, "{report}");
    }

    /// O plano não tem mais rede de tamanho: uma onda com mais de três
    /// tarefas no arquivo — nascida assim antes da regra sair, ou por uma
    /// edição de fora do binário — passa sem bloqueio nenhum. É o caso que a
    /// recusa `wave-too-big` bloqueava antes desta onda, e nenhuma outra
    /// recusa do plano a substitui.
    #[test]
    fn a_wave_grown_big_outside_the_write_is_not_blocked_by_the_plan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        let wave = write(root, Some("x"), "wave", json!({"n": 1, "text": "Somar.", "criteria": [crit], "done_when": "passa", "origin": said}));
        let first_id = id_of(&wave) + 1;
        for i in 0..4 {
            append_raw(
                root,
                "x",
                "task",
                json!({"wave": 1, "text": format!("Tarefa {i}."),
                "files": [{"path": "src/a.rs"}], "origin": said}),
                first_id + i,
            );
        }

        let report = plan(root, "x");
        assert!(!reasons(&report, "blocking").contains(&"wave-too-big".to_string()), "{report}");
    }

    /// O plano segura a pergunta de aprovação quando a prova de um critério
    /// não é um comando de verdade — a frase no lugar do comando, os
    /// comandos ligados por `;` ou a busca sozinha sem `!` —, e a recusa
    /// nomeia o critério e o termo que falhou. O critério com prova boa não
    /// trava o plano.
    #[test]
    fn a_criterion_proof_that_is_not_a_real_command_holds_the_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);
        assert_eq!(plan(root, "x")["ok"], json!(true), "a prova boa não trava o plano");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let mut next = log.max_id();
        for proof in ["sai vazio", "git --version ; git --help", "git grep -n x"] {
            next += 1;
            append_raw(
                root,
                "x",
                "criterion",
                json!({"when": "a", "then": "b", "proof": proof, "form": "ubiquitous",
                "origin": said}),
                next,
            );
        }

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        let blocking = reasons(&report, "blocking");
        for reason in ["proof-program-unknown", "proof-chained-by-semicolon", "proof-search-not-negated"] {
            assert!(blocking.contains(&reason.to_string()), "{reason}: {report}");
        }
        let hints: Vec<String> =
            report["blocking"].as_array().cloned().unwrap_or_default().iter().filter_map(|f| f["hint"].as_str().map(str::to_string)).collect();
        assert!(hints.iter().any(|h| h.starts_with("MSTD-CRIT-") && h.contains("sai")), "{hints:?}");
    }

    /// O critério gravado cru, sem passar pela gravação, com uma prova que
    /// cita um alvo de teste ou um arquivo que o projeto não tem, segura a
    /// pergunta de aprovação: os manifestos e os arquivos são lidos da raiz
    /// do plano. O alvo e o arquivo que uma tarefa do plano declara criar
    /// passam.
    #[test]
    fn a_criterion_proof_citing_what_the_project_lacks_holds_the_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"calc\"\n").unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("tests/soma.rs"), "").unwrap();
        write(
            root,
            Some("x"),
            "task",
            json!({"wave": 1, "text": "Cria os testes novos.",
            "files": [{"path": "tests/novo.rs", "new": true}, {"path": "web/novo.test.ts", "new": true}], "origin": said}),
        );

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let mut next = log.max_id();
        for proof in ["cargo test -p calc --test nope", "npx vitest run web/gone.test.ts", "cargo test -p calc --test novo", "npx vitest run web/novo.test.ts"] {
            next += 1;
            append_raw(root, "x", "criterion", json!({"when": "a", "then": "b", "proof": proof, "form": "ubiquitous", "origin": said}), next);
        }

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        let blocking = reasons(&report, "blocking");
        assert_eq!(blocking.iter().filter(|r| *r == "proof-test-target-missing").count(), 1, "{report}");
        assert_eq!(blocking.iter().filter(|r| *r == "proof-path-missing").count(), 1, "{report}");
        let hints = hints_of(&report, "blocking", "proof-path-missing");
        assert!(hints.iter().any(|hint| hint.contains("web/gone.test.ts")), "{hints:?}");
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
        write(
            root,
            Some("x"),
            "task",
            json!({"wave": 1, "text": "Some com o `Somador`.",
            "files": [{"path": "src/nao-existe.rs"}], "origin": said}),
        );
        write(
            root,
            Some("x"),
            "task",
            json!({"wave": 1, "text": "Escreve o resto.",
            "files": [{"path": "src/novo.rs", "new": true}], "origin": said}),
        );

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        let blocking = reasons(&report, "blocking");
        assert_eq!(blocking.iter().filter(|r| *r == "cited-file-missing").count(), 1, "{report}");
        assert!(reasons(&report, "warnings").contains(&"names-unchecked".to_string()), "{report}");
    }

    /// O fato de um ponto e o arquivo de uma tarefa citam `a.rs` só pelo
    /// nome, e um arquivo só do projeto tem esse nome, em `src`: a gravação do
    /// ponto recusa, a conferência do plano trava, e as duas mensagens dizem o
    /// caminho inteiro dele.
    #[test]
    fn a_refused_citation_names_the_only_file_with_that_name() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");

        let fact = json!({"block": "limits", "gap": "tamanho", "from": "gap", "status": "open",
            "facts": [{"text": "o defeito mora aqui", "source": "a.rs:1"}], "origin": said});
        let refused = write(root, Some("x"), "point", fact);
        assert_eq!(refused["reason"], json!("cited-file-missing"), "{refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains("src/a.rs"), "{refused}");

        let crit = criterion(root, "x", said);
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Somar.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Somar.", "files": [{"path": "a.rs"}], "origin": said}));
        let report = plan(root, "x");
        let hints = hints_of(&report, "blocking", "cited-file-missing");
        assert_eq!(hints.len(), 1, "{report}");
        assert!(hints[0].contains("a.rs") && hints[0].contains("src/a.rs"), "{hints:?}");
    }

    /// Só avisam, e a pergunta segue: arquivo fora do git, item de uma onda
    /// sem tarefa e contrato sem critério. A tarefa que diz no texto que não
    /// mexe em arquivo passa sem o campo.
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
        for reason in ["file-outside-git", "item-without-task", "contract-without-criterion"] {
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
        for (build, test, missing, present) in
            [(mustard_core::BUILD_COMMAND_FALLBACK, "make test", "buildCommand", "testCommand"), ("cargo build", "", "testCommand", "buildCommand")]
        {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let said = surveyed(root, "x");
            std::fs::write(root.join("mustard.json"), json!({"build_command": build, "test_command": test}).to_string()).unwrap();
            sound_plan(root, "x", said);

            let report = plan(root, "x");
            assert_eq!(report["ok"], json!(true), "{report}");
            let hints = hints_of(&report, "warnings", "command-not-declared");
            assert!(hints.iter().any(|h| h.contains(missing)), "{missing}: {hints:?}");
            assert!(!hints.iter().any(|h| h.contains(present)), "{present} está declarado: {hints:?}");

            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let built = prompts(root, "x", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
            assert_eq!(built.len(), 1, "{built:?}");
            let text = &built[0].text;
            assert!(!text.contains(mustard_core::BUILD_COMMAND_FALLBACK), "{text}");
            assert_eq!(text.contains("Compile com"), missing != "buildCommand", "{text}");
        }
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

    /// A tarefa de onda que já tem registro de entrega não é conferida: o
    /// arquivo citado, a tarefa sem arquivo e a coerência olham só as ondas
    /// que ainda vêm.
    #[test]
    fn a_task_of_a_delivered_wave_is_not_checked_anymore() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(
            root,
            Some("x"),
            "wave",
            json!({"n": 1, "text": "Os ganchos da sessão.",
            "criteria": [crit], "done_when": "passa", "origin": said}),
        );
        write(
            root,
            Some("x"),
            "wave",
            json!({"n": 2, "text": "A página da spec.",
            "criteria": [crit], "done_when": "passa", "origin": said}),
        );
        // A mesma tarefa quebrada nas duas ondas: cita um arquivo que não
        // existe e não está marcado como novo.
        for wave in [1, 2] {
            write(
                root,
                Some("x"),
                "task",
                json!({"wave": wave, "text": "Somar dois números.",
                "files": [{"path": "src/somar.rs"}], "origin": said}),
            );
        }
        let record = write(root, Some("x"), "delivered", json!({"wave": 1, "text": "A onda 1 saiu.", "files": ["src/somar.rs"]}));
        assert_eq!(record["ok"], json!(true), "{record}");

        let report = plan(root, "x");
        let blocking: Vec<String> =
            report["blocking"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(|f| f["hint"].as_str().map(str::to_string)).collect();
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
        write(
            root,
            Some("x"),
            "decision",
            json!({"text": "Quem decidiu sozinho.", "keys": ["registro"],
            "why": "registro da conversa", "no_code": "é registro de processo, não vira código", "waves": [1], "origin": said}),
        );
        write(
            root,
            Some("x"),
            "decision",
            json!({"text": "A rodada formata os arquivos dela.", "keys": ["formatador"],
            "why": "o commit sai formatado", "waves": [1], "origin": said}),
        );
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
        write(
            root,
            Some("x"),
            "task",
            json!({"wave": 1, "text": "Mexer.", "files": [{"path": "src/a.rs"}],
            "covers": [covered], "origin": said}),
        );
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let unowned: Vec<String> = wave_prompt::unowned(&log).iter().map(|e| codes[&e.id].clone()).collect();
        assert_eq!(unowned, ["MSTD-DEC-0001", "MSTD-DEC-0005"], "the two items really have no owner");

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "the item without owner does not hold the plan: {report}");
        assert_eq!(report["phase"], json!("plan"), "{report}");
        let every: Vec<String> =
            ["blocking", "warnings"].iter().flat_map(|field| report[*field].as_array().cloned().unwrap_or_default()).map(|f| f.to_string()).collect();
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
    /// recusa pede para nomear o arquivo, sem sugerir nenhum: o texto da
    /// tarefa nunca vira busca no mapa. A que declara no texto que não mexe em
    /// arquivo passa sem o campo.
    #[test]
    fn a_code_task_without_a_file_blocks_the_plan_and_asks_for_the_file_without_suggesting_one() {
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
        assert!(hint.contains("Nomeie o arquivo na tarefa"), "a recusa pede o arquivo: {hint}");
        assert!(!hint.contains("sugere"), "a recusa não sugere arquivo: {hint}");
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
        let messages: Vec<String> =
            report["warnings"].as_array().cloned().unwrap_or_default().iter().filter_map(|f| f["hint"].as_str().map(str::to_string)).collect();
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
        write(
            root,
            Some("x"),
            "wave",
            json!({"n": 1, "text": "Uma.", "criteria": [crit],
            "done_when": "passa", "origin": said}),
        );
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer.", "files": [{"path": "src/nao-existe.rs"}], "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        assert_eq!(reasons(&report, "blocking").first().map(String::as_str), Some("cited-file-missing"), "{report}");
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
        let note = write(root, Some("x"), "note", json!({"text":"A senha do banco: S3nh4F0rte","keys":["banco"],"origin":said}));
        let code = note["code"].as_str().unwrap();
        let report = plan(root, "x");
        assert_eq!(report["ok"], true, "{report}");
        assert_eq!(report["withheld"], json!([code]));
        assert!(report["warnings"].as_array().unwrap().iter().any(|w| w["reason"] == "page-check" && w["hint"].as_str().unwrap().contains("write purge")));
        assert!(report.get("copy").is_none() && report.get("publish").is_none());
        assert!(!root.join(".claude/mustard/publications").exists());
        let export = crate::commands::panel::prepare_publication(root, "x", false);
        let body = std::fs::read_to_string(export["database"].as_str().unwrap()).unwrap();
        assert!(!body.contains("S3nh4F0rte") && !body.contains("senha"));
        assert_eq!(export["published"], false);
        assert_eq!(write(root, Some("x"), "purge", json!({"targets":[code],"reason":"secret"}))["ok"], true);
        assert!(plan(root, "x").get("withheld").is_none());
    }

    /// A dica de aprovar do plano lê a opção de aprovar do catálogo, a mesma
    /// que a testemunha da aprovação reconhece, nos dois idiomas.
    #[test]
    fn the_approval_hint_reads_the_option_from_the_catalog() {
        for (lang, config) in [(Locale::PtBr, "{}".to_string()), (Locale::EnUs, r#"{"language":{"text":"en-US"}}"#.to_string())] {
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
            write(
                root,
                Some("x"),
                "wave",
                json!({"n": n, "text": "Mexer no código.", "criteria": [crit],
                "done_when": "passa", "origin": said}),
            );
        }
    }

    /// A tarefa sem nota não segura a aprovação, não soma nada e o texto da
    /// pergunta não cita a nota do Scrum: o plano segue só com o que o
    /// catálogo já dizia antes de a nota existir.
    #[test]
    fn plan_neither_asks_for_nor_adds_a_note() {
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

    /// Antes da aprovação, o plano não copia a spec nem publica página: a
    /// resposta traz só a pergunta. Uma spec que uma versão antiga publicou
    /// ainda no plano recebe a mesma resposta.
    #[test]
    fn approving_a_spec_neither_copies_nor_publishes_it() {
        for legacy in [false, true] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let said = surveyed(root, "x");
            sound_plan(root, "x", said);
            if legacy {
                let page = json!({"page":"spec","milestone":"approval","ok":true,"url":"https://claude.ai/code/artifact/old"});
                assert_eq!(write(root, Some("x"), "publish", page)["ok"], true);
            }
            let report = plan(root, "x");
            assert_eq!(report["ok"], true, "{report}");
            assert!(report.get("copy").is_none() && report.get("publish").is_none());
            assert!(!root.join(".claude/spec/x/copy").exists());
            assert!(!root.join(".claude/mustard/publications").exists());
            assert!(report["next"].as_str().unwrap().contains("Aprovar esta spec?"));
        }
    }

    /// A gravação da aprovação, pela mesma porta da testemunha.
    fn approve(root: &Path, spec: &str) -> Result<(), Refusal> {
        let mut draft = Map::new();
        draft.insert("phase".to_string(), json!("approved"));
        draft.insert("author".to_string(), json!("user"));
        draft.insert("witness".to_string(), json!({ "question": "Aprovar esta spec?", "answer": "Aprovar" }));
        record(root, spec, "state", draft, PhaseWriter::Witness).map(|_| ())
    }

    /// O plano passa, uma tarefa nova cita um arquivo que não existe, e o
    /// plano roda de novo e recusa: o "Aprovar" não grava nada. Tirada a
    /// tarefa e passado o plano de novo, a aprovação grava.
    #[test]
    fn approval_after_a_refused_replan_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);
        assert_eq!(plan(root, "x")["ok"], json!(true));

        let bad = id_of(&write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer no que falta.", "files": [{"path": "src/falta.rs"}], "origin": said})));
        let refused = plan(root, "x");
        assert_eq!(refused["ok"], json!(false), "{refused}");
        assert!(approve(root, "x").is_err(), "a refused plan is never approved");
        assert_eq!(State::from_log(&store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()).phase, Some("plan"));

        write(root, Some("x"), "remove", json!({"targets": [bad], "reason": "Cita arquivo que não existe."}));
        assert_eq!(plan(root, "x")["ok"], json!(true));
        assert!(approve(root, "x").is_ok(), "the plan passed again and nothing changed since");
    }

    /// O plano passa e depois some do disco um arquivo citado: o plano roda
    /// de novo e recusa sem nenhuma tarefa nova gravada, e o "Aprovar" não
    /// grava nada.
    #[test]
    fn approval_after_a_replan_refused_by_the_disk_alone_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);
        assert_eq!(plan(root, "x")["ok"], json!(true));

        std::fs::remove_file(root.join("src/a.rs")).unwrap();
        let refused = plan(root, "x");
        assert_eq!(refused["ok"], json!(false), "{refused}");
        // The refusal notes its findings and holds the approval, and its hint says so.
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(!hint.contains("Nada foi gravado") && hint.contains("anotados") && hint.contains("travada"), "{refused}");
        assert!(approve(root, "x").is_err(), "the last plan did not pass");
    }

    /// O plano passa e depois uma tarefa nova é gravada sem o plano rodar de
    /// novo: o "Aprovar" não grava nada até o plano conferir a tarefa.
    #[test]
    fn approval_after_a_plan_change_without_replan_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);
        assert_eq!(plan(root, "x")["ok"], json!(true));

        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer em b.", "files": [{"path": "src/b.rs"}], "origin": said}));
        let refusal = approve(root, "x").expect_err("the plan has not seen the new task");
        assert!(refusal.message(Locale::PtBr).contains("run plan"), "{}", refusal.message(Locale::PtBr));

        assert_eq!(plan(root, "x")["ok"], json!(true));
        assert!(approve(root, "x").is_ok());
    }

    /// Um plano sem tarefa nenhuma não passa: não há o que aprovar.
    #[test]
    fn a_plan_without_any_task_is_not_ready() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        criterion(root, "x", said);

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        assert!(reasons(&report, "blocking").contains(&"plan-without-task".to_string()), "{report}");
    }

    /// Uma tarefa sem onda que cobre o critério `crit`, com o assunto
    /// `subject` que a formação deu, quando há, e as dependências `depends_on`.
    fn backlog_task(root: &Path, said: u64, crit: u64, title: &str, subject: Option<&str>, depends_on: &[u64]) -> u64 {
        let mut task = json!({"title": title, "text": "Escrever a soma.", "files": [{"path": "src/a.rs"}], "depends_on": depends_on,
            "covers": [crit], "origin": said});
        if let Some(name) = subject {
            task["group"] = json!({"name": name, "position": 1, "formed": true});
        }
        id_of(&write(root, Some("x"), "task", task))
    }

    /// Outra spec do mesmo projeto, gravada crua com as horas: a onda 1 saiu,
    /// saiu de novo e entrou no commit 30 minutos depois da primeira saída; a
    /// 2 levou 10 minutos; a 3, 50; e a 4 saiu e ainda não tem commit.
    fn measured_spec(root: &Path) {
        let lines = [
            r#"{"v":1,"id":1,"at":"2026-10-01T10:00:00-03:00","type":"send","wave":1,"role":"wave","text":"Pedido."}"#,
            r#"{"v":1,"id":2,"at":"2026-10-01T10:20:00-03:00","type":"send","wave":1,"role":"wave","text":"Reenvio."}"#,
            r#"{"v":1,"id":3,"at":"2026-10-01T10:30:00-03:00","type":"commit","waves":[1],"sha":"a1"}"#,
            r#"{"v":1,"id":4,"at":"2026-10-01T11:00:00-03:00","type":"send","wave":2,"role":"wave","text":"Pedido."}"#,
            r#"{"v":1,"id":5,"at":"2026-10-01T11:00:00-03:00","type":"send","wave":3,"role":"wave","text":"Pedido."}"#,
            r#"{"v":1,"id":6,"at":"2026-10-01T11:10:00-03:00","type":"commit","waves":[2],"sha":"b2"}"#,
            r#"{"v":1,"id":7,"at":"2026-10-01T11:50:00-03:00","type":"commit","waves":[3],"sha":"c3"}"#,
            r#"{"v":1,"id":8,"at":"2026-10-01T12:00:00-03:00","type":"send","wave":4,"role":"wave","text":"Pedido."}"#,
        ];
        let folder = root.join(".claude/spec/antiga");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("spec.ndjson"), lines.join("\n") + "\n").unwrap();
    }

    /// Antes da pergunta, a resposta do plano diz quantas ondas a formação
    /// deu, quantas etapas a espera entre elas faz e quanto cada onda levou
    /// neste projeto, da saída ao commit, com o total como estimativa.
    #[test]
    fn the_plan_shows_how_many_waves_and_how_long_each_wave_took_here() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        let first = backlog_task(root, said, crit, "Somar os inteiros", Some("A soma"), &[]);
        backlog_task(root, said, crit, "Somar os decimais", Some("A soma"), &[]);
        backlog_task(root, said, crit, "Mostrar a soma", Some("A tela"), &[first]);
        measured_spec(root);

        let report = plan(root, "x");

        assert_eq!(report["ok"], json!(true), "{report}");
        let forecast = &report["forecast"];
        assert_eq!((&forecast["waves"], &forecast["stages"], &forecast["formed"]), (&json!(2), &json!(2), &json!(true)), "{report}");
        assert_eq!(forecast["wave_minutes"], json!({"measured": 3, "shortest": 10, "median": 30, "longest": 50}), "{report}");
        assert_eq!(forecast["estimate_minutes"], json!(60), "{report}");
        let text = forecast["text"].as_str().unwrap_or_default();
        for said in ["formação: 2", "elas: 2", "commit: 3", "10 min", "30 min", "50 min", "etapas: 60 min"] {
            assert!(text.contains(said), "{said}: {text}");
        }
        assert!(report["next"].as_str().is_some_and(|next| next.contains("`forecast`")), "{report}");
    }

    /// Sem a formação, cada tarefa sem assunto conta como uma onda sozinha, e
    /// o bloco diz o número de ondas como teto, sem dizer que ele veio da
    /// formação.
    #[test]
    fn without_the_formation_the_plan_says_the_waves_are_a_ceiling_of_one_per_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        let first = backlog_task(root, said, crit, "Somar os inteiros", None, &[]);
        backlog_task(root, said, crit, "Mostrar a soma", None, &[first]);

        let report = plan(root, "x");

        let forecast = &report["forecast"];
        assert_eq!((&forecast["waves"], &forecast["stages"], &forecast["formed"]), (&json!(2), &json!(2), &json!(false)), "{report}");
        let text = forecast["text"].as_str().unwrap_or_default();
        assert!(text.contains("no máximo: 2, uma por tarefa sem assunto") && text.contains("teto"), "{text}");
        assert!(!text.contains("pela formação"), "{text}");
    }

    /// A retomada da spec no plano, na resposta e no bloco do começo da
    /// sessão, manda conferir o plano pelo próximo comando e dizer o bloco
    /// das ondas e do tempo antes da pergunta de aprovação; o comando que ela
    /// dá é o do plano, que traz esse bloco.
    #[test]
    fn resuming_in_the_plan_phase_says_the_forecast_before_the_approval_question() {
        use crate::commands::flow::resume::{ResumeOpts, resume_block, resume_for};
        use crate::hooks::session::session_start_inject::MAX_BYTES;

        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);
        assert_eq!(plan(root, "x")["phase"], json!("plan"));

        let resumed = resume_for(&ResumeOpts { root: root.to_path_buf(), spec: Some("x".into()) }, None);
        assert_eq!(resumed["command"], json!("mustard-rt run plan --spec x"), "{resumed}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let block = resume_block("x", &log, &crate::commands::spec_events::conversation::observed(root, "x", &log), Locale::PtBr, MAX_BYTES);
        let question = translate("approval.question", Locale::PtBr);
        for told in [resumed["next"].as_str().unwrap_or_default(), block.as_str()] {
            let before = told.find("`forecast`").zip(told.find(question)).is_some_and(|(forecast, asked)| forecast < asked);
            assert!(before, "{told}");
        }
        let again = plan(root, "x");
        assert!(again["forecast"]["text"].as_str().is_some_and(|text| !text.is_empty()), "{again}");
    }

    /// Sem onda medida no projeto, o bloco diz que ainda não há medida, sem
    /// número de tempo nenhum.
    #[test]
    fn a_project_without_a_measured_wave_says_there_is_no_measure_yet() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);

        let report = plan(root, "x");

        let forecast = &report["forecast"];
        assert_eq!((&forecast["waves"], &forecast["stages"]), (&json!(1), &json!(1)), "{report}");
        assert!(forecast["wave_minutes"].is_null() && forecast.get("estimate_minutes").is_none(), "{report}");
        let text = forecast["text"].as_str().unwrap_or_default();
        assert!(text.contains(translate("plan.forecast.no_measure", Locale::PtBr)) && !text.contains("min"), "{text}");
    }

    /// Com uma sessão conduzindo e a lista ainda sem formação, o plano pede a
    /// leitura ao agente de análise e para antes da pergunta, sem gravar a
    /// fase do plano.
    #[test]
    fn the_plan_asks_for_the_wave_formation_before_the_approval_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        backlog_task(root, said, crit, "Somar os inteiros", None, &[]);
        backlog_task(root, said, crit, "Mostrar a soma", None, &[]);

        let report = plan_for(&PlanOpts { root: root.to_path_buf(), spec: Some("x".into()) }, Some("session"));

        assert!(report["formation"]["delegation"]["request"]["prompt"].as_str().is_some_and(|prompt| prompt.contains("Mostrar a soma")), "{report}");
        assert_eq!(report["next"], json!(translate("plan.formation_pending", Locale::PtBr)), "{report}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(State::from_log(&log).phase, Some("survey"), "{report}");
    }
}
