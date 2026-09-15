//! `mustard-rt run plan [--spec <nome>]` — o passo do plano: monta o pedido
//! de cada onda, confere tudo e leva a spec do levantamento para o plano.
//!
//! É a porta entre o levantamento e a aprovação. Depois que a especificação,
//! as ondas e as tarefas estão gravadas, este comando monta o pedido de cada
//! onda a partir dos eventos — com as lições e as skills —, confere o plano
//! contra o código real, refaz a página e o índice, grava a fase `plan` pela
//! mesma porta de gravação de fase das outras, e responde o próximo passo:
//! publique a página e faça a pergunta de aprovação.
//!
//! **O que trava** e segura a pergunta até ser corrigido: ponto do
//! levantamento aberto; erro de montagem do plano (ciclo entre ondas, e
//! tarefa ou dependência apontando uma onda que não existe); pedido de onda
//! acima do teto de linhas; skill que a conferência recusa; arquivo citado
//! que não existe e não está marcado como novo.
//!
//! **O que só avisa**, e a decisão fica com quem aprova: arquivo citado fora
//! do git (um agente noutra sessão ou máquina não o vê); nome citado que o
//! mapa não acha; ondas que saem na mesma rodada e dividem arquivo; item
//! combinado que nenhuma tarefa cobre; contrato que nenhum critério cita; e
//! tarefa sem arquivo.
//!
//! Quando a publicação anterior falhou, a resposta já traz o `scp` pronto
//! para o usuário copiar a página para a máquina dele: a aprovação não fica
//! presa a uma dependência de fora.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::citation::{self, CitationWorld, Finding};
use mustard_core::domain::project_map::MapRefusal;
use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState, State};
use mustard_core::domain::survey::{open_points, open_refusal};
use mustard_core::io::citation::DiskWorld;
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{prompts, WavePrompt};
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

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
    /// Uma tarefa que não diz em que arquivo mexe.
    TaskWithoutFile { task: String },
}

impl PlanFinding {
    /// `true` para o achado que segura a pergunta de aprovação.
    fn blocks(&self) -> bool {
        match self {
            Self::Refused(_)
            | Self::Skill { .. }
            | Self::WaveLoop { .. }
            | Self::DependsOnMissing { .. }
            | Self::TaskWithoutWave { .. } => true,
            Self::Cited { finding, .. } => finding.is_refusal(),
            Self::SharedFile { .. }
            | Self::FileOutsideGit { .. }
            | Self::ItemWithoutTask { .. }
            | Self::ContractWithoutCriterion { .. }
            | Self::TaskWithoutFile { .. } => false,
        }
    }

    /// A razão curta, estável, para quem lê a saída por máquina.
    fn reason(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.reason().to_string(),
            Self::Skill { refusal, .. } => refusal.reason().to_string(),
            Self::SharedFile { .. } => "waves-share-a-file".into(),
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
            Self::TaskWithoutFile { task } => fill("plan.task_without_file", &[("{task}", task.clone())]),
        }
    }

    fn to_value(&self, lang: Locale) -> Value {
        json!({ "reason": self.reason(), "hint": self.message(lang) })
    }
}

/// O núcleo testável de [`run`]. A sessão vem do ambiente. Nunca entra em
/// pânico.
pub(crate) fn plan_at(opts: &PlanOpts) -> Value {
    plan_for(opts, session_from_env().as_deref(), std::env::var("SSH_CONNECTION").ok().as_deref())
}

/// [`plan_at`] com a sessão e a conexão recebidas, que é como um teste as
/// escolhe.
pub(crate) fn plan_for(opts: &PlanOpts, session: Option<&str>, ssh: Option<&str>) -> Value {
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

    let built = prompts(&project.root, &spec, &log, lang);
    let findings = check(&opts.root, &project.root, &spec, &log, &built);
    let blocking: Vec<Value> =
        findings.iter().filter(|f| f.blocks()).map(|f| f.to_value(lang)).collect();
    let warnings: Vec<Value> =
        findings.iter().filter(|f| !f.blocks()).map(|f| f.to_value(lang)).collect();
    let waves: Vec<Value> = built
        .iter()
        .map(|p| json!({ "wave": p.wave, "lines": p.lines, "skills": p.stale_skills }))
        .collect();

    if !blocking.is_empty() {
        return json!({
            "ok": false, "spec": spec, "reason": "plan-not-ready",
            "hint": translate("plan.not_ready", lang).replace("{count}", &blocking.len().to_string()),
            "waves": waves, "blocking": blocking, "warnings": warnings,
        });
    }

    // A fase passa pela mesma porta de gravação de fase das outras, e ela já
    // refaz a página, o `.md` e a linha da spec no índice. Uma spec que já
    // está em plano não grava nada e tem as páginas refeitas aqui.
    let from = State::from_log(&log).phase.unwrap_or("-").to_string();
    let recorded = if from == "plan" {
        if let Err(refusal) = crate::commands::spec_events::pages::refresh(&project.root, &spec, lang) {
            return refuse(&refusal);
        }
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

    let mut report = json!({
        "ok": true, "spec": spec, "phase": "plan", "from": from,
        "waves": waves, "warnings": warnings,
        "next": translate("plan.next", lang),
    });
    if let Some(id) = recorded {
        report["id"] = json!(id);
    }
    if let Some(command) = fallback_copy(&project.root, &spec, &log, ssh) {
        report["copy"] = json!(command);
        report["next"] = json!(format!("{} {}", translate("plan.next", lang), translate("plan.copy", lang)));
    }
    report
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

    // Cada pedido cabe no teto de linhas, e cada skill nomeada passa.
    for prompt in built {
        if let Some(refusal) = &prompt.too_long {
            out.push(PlanFinding::Refused(refusal.clone()));
        }
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
    let mut cited: Vec<String> = Vec::new();
    for task in &tasks {
        let code = code_of(task);
        let files = declared_files(task);
        if files.is_empty() {
            out.push(PlanFinding::TaskWithoutFile { task: code.clone() });
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
    for task in &tasks {
        let code = code_of(task);
        for (path, new) in declared_files(task) {
            if !new && !tracked.contains(&path) {
                out.push(PlanFinding::FileOutsideGit { task: code.clone(), path });
            }
        }
    }

    // A cobertura: item combinado sem tarefa, contrato sem critério e tarefa
    // sem arquivo só avisam, e a decisão fica com quem aprova.
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
    for item in &agreed {
        if !covered.contains(&item.id) {
            out.push(PlanFinding::ItemWithoutTask { code: code_of(item) });
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

/// Os arquivos que uma tarefa declara: o caminho e se ela o marcou como novo.
fn declared_files(task: &SpecEvent) -> Vec<(String, bool)> {
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

/// O comando que copia a página para a máquina do usuário, quando a última
/// publicação falhou. Numa sessão por SSH ele vem pronto, com o endereço do
/// servidor e o usuário da sessão; fora dela, vem o caminho do arquivo.
fn fallback_copy(root: &Path, spec: &str, log: &SpecLog, ssh: Option<&str>) -> Option<String> {
    let failed = log
        .block(BlockQuery::Block(Block::State))
        .into_iter()
        .rfind(|e| e.event_type == "publish")
        .is_some_and(|e| e.fields.get("ok").and_then(Value::as_bool) == Some(false));
    if !failed {
        return None;
    }
    let page = mustard_core::ClaudePaths::for_project(root).ok()?.for_spec(spec).ok()?.spec_html_path();
    let shown = page.to_string_lossy().replace('\\', "/");
    let server = ssh.and_then(|line| line.split_whitespace().nth(2).map(str::to_string));
    match server {
        Some(server) => {
            let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
            let who = if user.is_empty() { String::new() } else { format!("{user}@") };
            Some(format!("scp {who}{server}:{shown} ."))
        }
        None => Some(shown),
    }
}

fn join(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

/// Roda o `plan` e imprime o relatório em JSON; sai com 1 na recusa.
pub fn run(opts: &PlanOpts) {
    let report = plan_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::flow::grill::{grill_for, GrillOpts};
    use crate::commands::spec_events::write::{record_open, write_at, WriteOpts};
    use std::process::Command;
    use tempfile::tempdir;

    const GOAL: &str = "Travar o merge enquanto houver pendência aberta.";

    fn write(root: &Path, spec: Option<&str>, event_type: &str, body: Value) -> Value {
        write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: spec.map(str::to_string),
            event_type: event_type.into(),
            json: body.to_string(),
        })
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
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test", "origin": said})))
    }

    /// Uma onda com uma tarefa, num plano que passa em tudo.
    fn sound_plan(root: &Path, spec: &str, said: u64) {
        let crit = criterion(root, spec, said);
        write(root, Some(spec), "wave", json!({"n": 1, "text": "Somar.", "criteria": [crit], "done_when": "A suíte passa.", "origin": said}));
        write(root, Some(spec), "task", json!({"wave": 1, "text": "Escrever a soma.", "files": [{"path": "src/a.rs"}], "origin": said}));
    }

    fn plan(root: &Path, spec: &str) -> Value {
        plan_for(&PlanOpts { root: root.to_path_buf(), spec: Some(spec.into()) }, None, None)
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
    /// página e o `.md` saem refeitos, e a resposta manda publicar a página e
    /// fazer a pergunta.
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
        assert_eq!(report["next"], json!(translate("plan.next", Locale::PtBr)));
        assert!(report["copy"].is_null(), "sem publicação falha, nada de copiar: {report}");
        assert!(root.join(".claude/spec/x/spec.html").is_file());

        // A linha da spec no índice sai refeita junto com a página.
        let index = std::fs::read_to_string(root.join(".claude/spec/index.ndjson")).unwrap();
        assert!(index.contains("\"phase\":\"plan\""), "{index}");

        // Rodar de novo não grava fase nenhuma, refaz a página e o índice, e
        // continua respondendo o mesmo.
        std::fs::remove_file(root.join(".claude/spec/x/spec.html")).unwrap();
        std::fs::remove_file(root.join(".claude/spec/index.ndjson")).unwrap();
        let again = plan(root, "x");
        assert_eq!(again["ok"], json!(true), "{again}");
        assert_eq!(again["from"], json!("plan"));
        assert!(again["id"].is_null(), "{again}");
        assert!(root.join(".claude/spec/x/spec.html").is_file(), "a página volta");
        let rebuilt = std::fs::read_to_string(root.join(".claude/spec/index.ndjson")).unwrap();
        assert_eq!(rebuilt, index, "o índice volta igual");
    }

    /// Cada linha de cada pedido aparece na página da spec, sem exceção, as
    /// instruções fixas incluídas: é por essa página que a spec é aprovada.
    #[test]
    fn every_line_of_every_request_shows_up_on_the_page() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "rule", json!({"text": "No máximo 3 tentativas de compilação.",
            "example": "a quarta para", "keys": ["tentativas"], "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Somar.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Escrever a soma.", "files": [{"path": "src/a.rs"}], "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 2, "text": "Subtrair.", "criteria": [crit], "done_when": "passa", "depends_on": [1], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Escrever a subtração.", "files": [{"path": "src/b.rs"}], "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "{report}");
        let page = std::fs::read_to_string(root.join(".claude/spec/x/spec.html")).unwrap();
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let built = prompts(root, "x", &log, Locale::PtBr);
        assert_eq!(built.len(), 2, "duas ondas, dois pedidos");
        for prompt in &built {
            assert!(prompt.lines > 0);
            for line in prompt.text.lines().filter(|l| !l.trim().is_empty()) {
                let escaped = crate::report::escape(line);
                assert!(
                    page.contains(&escaped),
                    "a onda {} não mostra a linha {line:?}",
                    prompt.wave
                );
            }
        }
        // As instruções fixas, que todo agente recebe, estão entre elas.
        assert!(page.contains(translate("prompt.fixed", Locale::PtBr).lines().next().unwrap()));
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

    /// Um pedido acima do teto de linhas trava a pergunta, e a mensagem diz
    /// quantas linhas ele tem.
    #[test]
    fn a_request_over_the_line_cap_blocks_the_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let crit = criterion(root, "x", said);
        let long = "uma linha do texto\n".repeat(600);
        write(root, Some("x"), "wave", json!({"n": 1, "text": long, "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Somar.", "files": [{"path": "src/a.rs"}], "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(false), "{report}");
        assert!(reasons(&report, "blocking").contains(&"wave-prompt-too-long".to_string()), "{report}");
        assert!(report["waves"][0]["lines"].as_u64().unwrap() > 500, "{report}");
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
    /// rodada dividindo arquivo, item sem tarefa, contrato sem critério e
    /// tarefa sem arquivo.
    #[test]
    fn the_advisory_findings_never_block_the_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        std::fs::write(root.join("src/fora.rs"), "fn tres() {}\n").unwrap();
        let crit = criterion(root, "x", said);
        write(root, Some("x"), "contract", json!({"text": "A barra tem duas linhas.", "example": "dev · x", "keys": ["barra"], "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 1, "text": "Uma.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 1, "text": "Mexer.", "files": [{"path": "src/a.rs"}, {"path": "src/fora.rs"}], "origin": said}));
        write(root, Some("x"), "wave", json!({"n": 2, "text": "Outra.", "criteria": [crit], "done_when": "passa", "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Mexer também.", "files": [{"path": "src/a.rs"}], "origin": said}));
        write(root, Some("x"), "task", json!({"wave": 2, "text": "Sem arquivo.", "origin": said}));

        let report = plan(root, "x");
        assert_eq!(report["ok"], json!(true), "{report}");
        let warnings = reasons(&report, "warnings");
        for reason in [
            "file-outside-git",
            "waves-share-a-file",
            "item-without-task",
            "contract-without-criterion",
            "task-without-file",
        ] {
            assert!(warnings.contains(&reason.to_string()), "{reason}: {report}");
        }
    }

    /// Quando a última publicação falhou, a conferência passa e a resposta
    /// traz o comando pronto: numa sessão por SSH, o `scp` com o endereço do
    /// servidor; fora dela, o caminho do arquivo.
    #[test]
    fn a_failed_publish_hands_the_copy_command_over() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        sound_plan(root, "x", said);
        let published = write(root, Some("x"), "publish", json!({"page": "spec",
            "milestone": "approval", "ok": false, "reason": "O claude.ai não respondeu.", "origin": said}));
        assert_eq!(published["ok"], json!(true), "{published}");

        let opts = PlanOpts { root: root.to_path_buf(), spec: Some("x".into()) };
        let over_ssh = plan_for(&opts, None, Some("10.0.0.2 51000 10.0.0.9 22"));
        assert_eq!(over_ssh["ok"], json!(true), "{over_ssh}");
        let copy = over_ssh["copy"].as_str().unwrap_or_default();
        assert!(copy.starts_with("scp ") && copy.contains("10.0.0.9:"), "{copy}");
        assert!(copy.ends_with("spec.html ."), "{copy}");
        assert!(over_ssh["next"].as_str().unwrap_or_default().contains(translate("plan.copy", Locale::PtBr)));

        let local = plan_for(&opts, None, None);
        let path = local["copy"].as_str().unwrap_or_default();
        assert!(path.ends_with("spec.html") && !path.starts_with("scp "), "{path}");
    }
}
