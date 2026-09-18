//! O pedido de cada onda, montado do disco: o arquivo de eventos da spec, o
//! banco de lições, os arquivos das skills que as tarefas nomeiam, os
//! comandos de compilar e testar que o projeto declara e o mapa do projeto,
//! que diz se ele tem uma parte Rust.
//!
//! A regra de escrever o pedido mora em `domain::wave_prompt`, sem disco;
//! aqui ficam só as leituras. Os dois leitores do pedido — o passo do plano,
//! que confere antes da pergunta de aprovação, e a página, que mostra cada
//! linha dele — passam por esta função, então não podem discordar sobre o
//! mesmo pedido.
//!
//! O pedido não copia o texto da skill: ele recomenda o arquivo dela, que o
//! agente da onda lê no disco. O texto ainda é lido aqui porque é nele que a
//! conferência acontece — a skill que cita um caminho que não existe ou passa
//! do tamanho máximo é recusada — e porque o "quando usar" da linha sai da
//! descrição do frontmatter. A skill cujo exemplo mudou no git depois dela
//! sai marcada como a revisar.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::domain::lessons::{defects_in_scope, in_scope, related_to_tasks, Scope};
use crate::domain::project_map::{check_skill, file_history, MapRefusal, ProjectMap};
use crate::domain::spec_events::{Block, BlockQuery, Refusal, SpecEvent, SpecLog};
use crate::domain::wave_prompt::{self, wave_files, Choice, Execution, Material, Skill, WaveCopy};
use crate::platform::i18n::Locale;

/// O pedido de uma onda, como o disco o entrega.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavePrompt {
    /// O número da onda.
    pub wave: u64,
    /// O texto do pedido, sempre: a página mostra mesmo o pedido recusado,
    /// que é justamente o que precisa ser visto antes da aprovação.
    pub text: String,
    /// O pedido do revisor da onda: a mesma lista de itens e os critérios que
    /// ele confere, mais os defeitos já vistos nos arquivos da onda.
    pub review: String,
    /// Quantas linhas ele tem.
    pub lines: usize,
    /// A recusa do teto de linhas, quando o pedido passa dele.
    pub too_long: Option<Refusal>,
    /// As skills que a conferência recusou, pelo nome.
    pub bad_skills: Vec<(String, MapRefusal)>,
    /// As skills que a onda usa e que precisam de revisão, pelo nome.
    pub stale_skills: Vec<String>,
}

/// As ondas que estão fora, como quem monta os pedidos as vê.
#[derive(Debug, Default)]
pub struct Flight {
    /// As ondas em andamento — a rodada conta também as que saem junto nela.
    /// O pedido de cada onda lista as outras, com os arquivos delas.
    pub running: BTreeSet<u64>,
    /// A cópia de cada onda que sai agora, que a rodada criou antes de gravar
    /// o envio. A cópia de uma onda que já saiu vem do envio gravado dela.
    pub copies: BTreeMap<u64, WaveCopy>,
    /// A escolha da análise antes do envio de cada onda que sai agora. A de
    /// uma onda que já saiu vem do envio gravado dela.
    pub choices: BTreeMap<u64, Choice>,
}

/// Os pedidos de todas as ondas do plano, em ordem de número, com as ondas
/// que estão fora em `flight`.
#[must_use]
pub fn prompts(root: &Path, spec: &str, log: &SpecLog, lang: Locale, flight: &Flight) -> Vec<WavePrompt> {
    let bank = crate::ClaudePaths::for_project(root)
        .ok()
        .and_then(|paths| crate::io::lessons::read(&paths.lessons_path()).ok().flatten());
    let map = crate::io::project_map::read(root).ok();
    let commands = crate::ProjectConfig::load(root).commands();
    let base = Execution {
        build: commands.build,
        test: commands.test,
        root: shown(root),
        rust: has_rust_part(map.as_ref()),
        ..Execution::default()
    };
    let context = Context { root, spec, log, bank: bank.as_ref(), map: map.as_ref(), base: &base, flight, lang };
    log.planned_waves().into_iter().map(|n| one(&context, n)).collect()
}

/// A pasta da cópia separada da onda `wave` da spec `spec`, dentro das cópias
/// do checkout `root`: a do agente da onda, ou a do revisor dela
/// (`review`). A rodada cria a primeira; o pedido da revisão manda criar a
/// segunda.
#[must_use]
pub fn copy_path(root: &Path, spec: &str, wave: u64, review: bool) -> PathBuf {
    let name = if review { format!("mustard-{spec}-{wave}-review") } else { format!("mustard-{spec}-{wave}") };
    crate::ClaudePaths::compose_unchecked(root).claude_dir().join("worktrees").join(name)
}

/// A pasta da cópia separada do revisor final da spec `spec`, ao lado das
/// cópias das ondas.
#[must_use]
pub fn final_copy_path(root: &Path, spec: &str) -> PathBuf {
    crate::ClaudePaths::compose_unchecked(root).claude_dir().join("worktrees").join(format!("mustard-{spec}-final-review"))
}

/// O pedido da revisão final do conjunto da spec `spec`: as ondas do plano
/// com as tarefas, a entrega mais nova de cada onda, os critérios e a cópia
/// do revisor, no commit mais novo da spec e na pasta de compilação que a
/// última onda enviada usou.
#[must_use]
pub fn final_review(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> String {
    let planned = log.planned_waves();
    let visible = log.block(BlockQuery::Block(Block::Waves));
    let block: Vec<&SpecEvent> = visible
        .iter()
        .copied()
        .filter(|e| matches!(e.event_type.as_str(), "wave" | "task") && e.wave().is_some_and(|n| planned.contains(&n)))
        .collect();
    let newest = log.last_by_wave("delivered");
    let own_delivered: Vec<&SpecEvent> = visible
        .iter()
        .copied()
        .filter(|e| e.event_type == "delivered")
        .filter(|e| e.wave().filter(|n| planned.contains(n)).and_then(|n| newest.get(&n)) == Some(&e.id))
        .collect();
    let criteria: Vec<&SpecEvent> =
        log.block(BlockQuery::Block(Block::Criteria)).into_iter().filter(|e| e.event_type == "criterion").collect();
    let commit = log
        .block(BlockQuery::Block(Block::Progress))
        .into_iter()
        .rev()
        .find_map(|e| (e.event_type == "commit").then(|| e.str_field("sha").map(str::to_string)).flatten());
    let last_sent = log.last_by_wave("send").into_iter().max_by_key(|(_, id)| *id).map(|(n, _)| n);
    let commands = crate::ProjectConfig::load(root).commands();
    let execution = Execution {
        build: commands.build,
        test: commands.test,
        commit,
        root: shown(root),
        review: WaveCopy {
            path: shown(&final_copy_path(root, spec)),
            build_dir: last_sent.and_then(|n| recorded_copy(log, n)).and_then(|copy| copy.build_dir),
        },
        rust: has_rust_part(crate::io::project_map::read(root).ok().as_ref()),
        ..Execution::default()
    };
    let material = Material {
        spec: spec.to_string(),
        block,
        criteria,
        own_delivered,
        execution,
        codes: log.codes(),
        ..Material::default()
    };
    wave_prompt::write_final_review(&material, lang)
}

/// A cópia gravada no envio mais novo da onda `wave`, com a pasta de
/// compilação dele. `None` quando esse envio não criou cópia.
#[must_use]
pub fn recorded_copy(log: &SpecLog, wave: u64) -> Option<WaveCopy> {
    let sent = log.last_by_wave("send").get(&wave).and_then(|id| log.get(*id))?;
    let path = sent.str_field("copy")?.to_string();
    Some(WaveCopy { path, build_dir: sent.str_field("build_dir").map(str::to_string) })
}

/// O mapa do projeto marca alguma parte dele como `cargo`? Só então os
/// pedidos mandam compilar na pasta de compilação da cópia, com o nome do
/// Cargo. Sem mapa, o Mustard não sabe que o projeto é Rust, e a frase fica
/// fora; a pasta continua escolhida, porque é ela a vaga das ondas que rodam
/// juntas.
fn has_rust_part(map: Option<&ProjectMap>) -> bool {
    map.is_some_and(|map| map.projects.iter().any(|part| part.kind == "cargo"))
}

/// Um caminho como o pedido e o envio gravado o mostram: sempre com barras
/// normais, que o terminal e o controle de versão aceitam nos três sistemas.
#[must_use]
pub fn shown(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// O que é igual para o pedido de todas as ondas de uma montagem.
struct Context<'a> {
    root: &'a Path,
    spec: &'a str,
    log: &'a SpecLog,
    bank: Option<&'a SpecLog>,
    map: Option<&'a ProjectMap>,
    /// Os comandos do projeto.
    base: &'a Execution,
    flight: &'a Flight,
    lang: Locale,
}

fn one(context: &Context, wave: u64) -> WavePrompt {
    let Context { root, spec, log, bank, map, lang, .. } = *context;
    // O que a montagem escolhe, com a escolha da análise antes do envio: a
    // que a rodada traz agora ou a gravada no envio da onda.
    let read = wave_prompt::dispatch_items(log, wave, context.flight.choices.get(&wave));
    let of_type = |name: &str| -> Vec<&SpecEvent> {
        read.iter().copied().filter(|e| e.event_type == name).collect()
    };
    let block: Vec<&SpecEvent> = read
        .iter()
        .copied()
        // O envio e o entregou são o REGISTRO de um pedido, não parte dele:
        // repeti-los dentro do pedido novo seria contar a mesma coisa duas
        // vezes. O entregou das ondas de que esta depende entra à parte.
        .filter(|e| e.block() == Some(Block::Waves) && !matches!(e.event_type.as_str(), "send" | "delivered"))
        .collect();
    let delivered: Vec<&SpecEvent> = read
        .iter()
        .copied()
        .filter(|e| e.event_type == "delivered" && e.wave() != Some(wave))
        .collect();
    // O revisor confere o que a onda entregou depois da última revisão dela:
    // no conserto, as entregas do conserto, mesmo as que vieram pela linha de
    // outra onda.
    let judged = log.verdicts_by_wave().get(&wave).and_then(|v| v.last()).map_or(0, |v| v.id);
    let own_delivered: Vec<&SpecEvent> = read
        .iter()
        .copied()
        .filter(|e| e.event_type == "delivered" && e.wave() == Some(wave) && e.id > judged)
        .collect();
    let agreed: Vec<&SpecEvent> = read
        .iter()
        .copied()
        .filter(|e| e.block() == Some(Block::Agreed))
        .collect();
    let specification: Vec<&SpecEvent> = read
        .iter()
        .copied()
        .filter(|e| e.block() == Some(Block::Specification))
        .collect();

    let files = wave_files(log, wave);
    let named = skills_named(log, wave);
    // Das lições que valem para a onda entram, de cada classe, só as mais
    // ligadas ao texto das tarefas: uma pasta com centenas delas passaria do
    // teto de linhas, e a lição sem palavra em comum com a tarefa só ocupa o
    // agente. O pedido da revisão segue a mesma conta.
    let tasks = tasks_text(log, wave);
    let lessons = bank
        .map(|bank| {
            let mut found: Vec<&SpecEvent> = Vec::new();
            for skill in std::iter::once(None).chain(named.iter().map(|s| Some(s.clone()))) {
                let scope = Scope { files: files.clone(), subproject: None, skill };
                for lesson in in_scope(bank, &scope) {
                    if !found.iter().any(|seen| seen.id == lesson.id) {
                        found.push(lesson);
                    }
                }
            }
            related_to_tasks(found, &tasks)
        })
        .unwrap_or_default();

    // O revisor recebe os defeitos já vistos nestes arquivos, que o agente da
    // onda não recebe: é olhando o erro que já aconteceu ali que ele começa.
    // Só os ligados às tarefas, como no pedido da onda.
    let defects = bank
        .map(|bank| {
            let scope = Scope { files: files.clone(), subproject: None, skill: None };
            related_to_tasks(defects_in_scope(bank, &scope), &tasks)
        })
        .unwrap_or_default();

    let mut skills = Vec::new();
    let mut bad_skills = Vec::new();
    let mut stale_skills = Vec::new();
    for name in &named {
        let Some((text, path)) = read_skill(root, &files, name) else {
            bad_skills.push((name.clone(), MapRefusal::SkillMissingPaths { paths: vec![skill_file(name)] }));
            continue;
        };
        if let Err(refusal) = check_skill(&text, |cited| cited_exists(root, cited)) {
            bad_skills.push((name.clone(), refusal));
            continue;
        }
        let stale = examples_changed_after(log, map, name);
        if stale {
            stale_skills.push(name.clone());
        }
        skills.push(Skill { name: name.clone(), when: when_to_use(&text), path, stale });
    }

    let material = Material {
        spec: spec.to_string(),
        wave,
        block,
        criteria: of_type("criterion"),
        specification,
        agreed,
        delivered,
        // A linha do conserto que a análise tirou do pedido sai também daqui.
        fix: wave_prompt::fix_lines(log, wave).into_iter().filter(|line| read.iter().any(|e| e.id == line.id)).collect(),
        own_delivered,
        execution: execution(context, wave),
        lessons,
        defects,
        skills,
        codes: log.codes(),
    };
    let text = wave_prompt::write(&material, lang);
    let review = wave_prompt::write_review(&material, lang);
    let lines = wave_prompt::count_lines(&text);
    let too_long =
        (lines > wave_prompt::MAX_LINES).then(|| wave_prompt::too_long(&material, lines, lang));
    WavePrompt { wave, text, review, lines, too_long, bad_skills, stale_skills }
}

/// As regras da execução da onda `wave`: os comandos do projeto, as outras
/// ondas em andamento com os arquivos delas, a cópia da onda — a que sai agora
/// ou a gravada no envio da que está em andamento — e a cópia do revisor, no
/// commit mais novo que leva a onda e na pasta de compilação que a cópia da
/// onda usou.
fn execution(context: &Context, wave: u64) -> Execution {
    let (log, flight) = (context.log, context.flight);
    let running = flight
        .running
        .iter()
        .filter(|n| **n != wave)
        .map(|n| (*n, wave_files(log, *n)))
        .collect();
    let commit = log
        .block(BlockQuery::Block(Block::Progress))
        .into_iter()
        .rev()
        .filter(|e| e.event_type == "commit" && e.ints("waves").contains(&wave))
        .find_map(|e| e.str_field("sha").map(str::to_string));
    let recorded = recorded_copy(log, wave);
    let copy = match flight.copies.get(&wave) {
        Some(copy) => Some(copy.clone()),
        None => recorded.clone().filter(|_| flight.running.contains(&wave)),
    };
    let review = WaveCopy {
        path: shown(&copy_path(context.root, context.spec, wave, true)),
        build_dir: recorded.and_then(|copy| copy.build_dir),
    };
    Execution { running, commit, copy, review, ..context.base.clone() }
}

/// O texto das tarefas de uma onda, uma por linha: a consulta que escolhe as
/// lições que o pedido da onda e o da revisão levam.
fn tasks_text(log: &SpecLog, wave: u64) -> String {
    log.block(BlockQuery::Wave(wave))
        .iter()
        .filter(|e| e.event_type == "task")
        .filter_map(|task| task.str_field("text"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// As skills que as tarefas de uma onda nomeiam, em ordem de nome.
fn skills_named(log: &SpecLog, wave: u64) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for task in log.block(BlockQuery::Wave(wave)).iter().filter(|e| e.event_type == "task") {
        if let Some(name) = task.str_field("skill").map(str::trim).filter(|s| !s.is_empty()) {
            names.insert(name.to_string());
        }
    }
    names.into_iter().collect()
}

/// O caminho de uma skill dentro do subprojeto que a tem.
fn skill_file(name: &str) -> String {
    format!(".claude/skills/{name}/SKILL.md")
}

/// O arquivo de uma skill, procurado nas pastas dos arquivos da onda e, por
/// último, na raiz do projeto: a skill mora no subprojeto em que a tarefa
/// mexe. Devolve o texto, que a conferência lê, e o caminho do arquivo a
/// partir da raiz do projeto, que é o que o pedido recomenda.
fn read_skill(root: &Path, files: &[String], name: &str) -> Option<(String, String)> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for file in files {
        let mut folder = root.join(file);
        while folder.pop() && folder.starts_with(root) {
            if !folders.contains(&folder) {
                folders.push(folder.clone());
            }
        }
    }
    if !folders.contains(&root.to_path_buf()) {
        folders.push(root.to_path_buf());
    }
    for folder in folders {
        let path = folder.join(".claude").join("skills").join(name).join("SKILL.md");
        if let Ok(text) = std::fs::read_to_string(&path) {
            return Some((text, from_root(root, &path)));
        }
    }
    None
}

/// Um caminho a partir da raiz do projeto, sempre com barras normais: o
/// pedido é lido por gente e por agente, e a barra invertida do Windows não
/// serve para nenhum dos dois.
fn from_root(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// Quando usar a skill, tirado da descrição do frontmatter dela e reduzido a
/// uma linha. A skill sem frontmatter, ou com a descrição vazia, entra no
/// pedido só com o nome e o caminho.
fn when_to_use(text: &str) -> String {
    let Ok(front) = crate::domain::skill::frontmatter::parse(text) else {
        return String::new();
    };
    front.description.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Um caminho citado pela skill existe? A skill pode citar só o fim do
/// caminho, então a busca é pelo que o projeto tem.
fn cited_exists(root: &Path, cited: &str) -> bool {
    if root.join(cited).exists() {
        return true;
    }
    crate::io::project_map::read(root)
        .is_ok_and(|map| map.modules.iter().any(|m| m.path.ends_with(cited)))
}

/// Algum exemplo que a skill usou mudou no git depois de ela ter sido
/// gravada?
fn examples_changed_after(log: &SpecLog, map: Option<&ProjectMap>, name: &str) -> bool {
    let Some(map) = map else { return false };
    let Some(event) = log
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "skill" && e.str_field("name") == Some(name))
    else {
        return false;
    };
    let Ok(at) = chrono::DateTime::parse_from_rfc3339(event.at()) else {
        return false;
    };
    let written = at.timestamp();
    event
        .fields
        .get("examples")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|example| example.get("path").and_then(Value::as_str))
        .any(|path| file_history(&map.history, path).is_some_and(|h| h.last_at > written))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{parse_log, render_line, stamp};
    use serde_json::json;
    use tempfile::tempdir;

    fn log_of(events: &[(&str, Value)]) -> SpecLog {
        let mut content = String::new();
        for (i, (event_type, body)) in events.iter().enumerate() {
            let mut map = crate::domain::spec_events::normalize(
                body.as_object().cloned().unwrap_or_default(),
                event_type,
            );
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, i as u64 + 1, None, "2026-09-15T10:00:00-03:00")));
            content.push('\n');
        }
        parse_log(&content)
    }

    fn plan_log() -> SpecLog {
        log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            (
                "task",
                json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}], "skill": "somar"}),
            ),
        ])
    }

    fn write_skill(root: &Path, subproject: &str, name: &str, text: &str) {
        let dir = root.join(subproject).join(".claude").join("skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), text).unwrap();
    }

    /// A skill que a tarefa nomeia entra no pedido pelo caminho do arquivo e
    /// pelo quando usar da descrição dela, sem o texto; e ela é procurada no
    /// subprojeto em que a tarefa mexe.
    #[test]
    fn the_skill_a_task_names_reaches_the_request_by_path_and_never_by_text() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_skill(
            root,
            "apps/rt",
            "somar",
            "---\nname: somar\ndescription: Use ao somar dois números no motor.\n---\n\n# Somar\n\nUm passo por linha.\n",
        );
        let built = prompts(root, "teste", &plan_log(), Locale::PtBr, &Flight::default());
        assert_eq!(built.len(), 1);
        assert!(built[0].bad_skills.is_empty(), "{:?}", built[0].bad_skills);
        assert!(
            built[0].text.contains("`apps/rt/.claude/skills/somar/SKILL.md`"),
            "{}",
            built[0].text
        );
        assert!(built[0].text.contains("Use ao somar dois números no motor."), "{}", built[0].text);
        assert!(!built[0].text.contains("Um passo por linha."), "{}", built[0].text);
    }

    /// A skill sem frontmatter entra mesmo assim: a linha fica só com o nome e
    /// o caminho, sem o trecho do quando usar.
    #[test]
    fn a_skill_without_frontmatter_still_reaches_the_request_by_path() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_skill(root, "apps/rt", "somar", "# Somar\n\nUm passo por linha.\n");
        let built = prompts(root, "teste", &plan_log(), Locale::PtBr, &Flight::default());
        assert!(built[0].bad_skills.is_empty(), "{:?}", built[0].bad_skills);
        assert!(
            built[0].text.contains("- **somar** — `apps/rt/.claude/skills/somar/SKILL.md`"),
            "{}",
            built[0].text
        );
    }

    /// A skill que cita um caminho que não existe, e a que passa do tamanho
    /// máximo, são recusadas, e o texto delas não entra no pedido.
    #[test]
    fn a_skill_citing_a_missing_path_or_too_long_is_refused() {
        let long = format!("# O molde\n\n{}", "linha do molde\n".repeat(600));
        for (text, reason) in [
            ("# O molde\n\nVeja `apps/rt/src/nao-existe.rs`.\n", "skill-missing-path"),
            (long.as_str(), "skill-too-long"),
        ] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            write_skill(root, "apps/rt", "somar", text);
            let built = prompts(root, "teste", &plan_log(), Locale::PtBr, &Flight::default());
            assert_eq!(built[0].bad_skills.len(), 1, "{reason}");
            assert_eq!(built[0].bad_skills[0].0, "somar");
            assert_eq!(built[0].bad_skills[0].1.reason(), reason);
            assert!(!built[0].bad_skills[0].1.message(Locale::EnUs).is_empty());
            assert!(!built[0].text.contains("O molde"), "{reason}: a skill recusada não entra");
        }
    }

    /// O pedido recomenda exatamente as skills que as tarefas nomeiam. Uma
    /// skill que está na prateleira do subprojeto e que nenhuma tarefa nomeia
    /// não entra: a escolha é sempre pela tarefa, nunca pelo caminho.
    #[test]
    fn only_the_skills_the_tasks_name_reach_the_request() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_skill(root, "apps/rt", "somar", "# Somar\n\nUm passo por linha.\n");
        write_skill(root, "apps/rt", "subtrair", "# Subtrair\n\nOutro molde da mesma pasta.\n");
        let built = prompts(root, "teste", &plan_log(), Locale::PtBr, &Flight::default());
        assert!(built[0].text.contains("skills/somar/SKILL.md"), "{}", built[0].text);
        assert!(!built[0].text.contains("skills/subtrair/SKILL.md"), "{}", built[0].text);
        assert!(!built[0].text.contains("MOLDS FOR THIS WAVE"), "{}", built[0].text);
    }

    /// Um item revisto entra na lista do pedido só na versão nova; a linha da
    /// antiga fica fora, e o número dela não aparece.
    #[test]
    fn a_revised_item_reaches_the_request_only_in_its_new_version() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_skill(root, "apps/rt", "somar", "# Somar\n\nUm passo por linha.\n");
        let log = log_of(&[
            ("limit", json!({"text": "O pedido cabe em 400 linhas.", "value": "400 linhas", "keys": ["pedido"]})),
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}], "skill": "somar"})),
            (
                "limit",
                json!({"text": "O pedido cabe em 500 linhas.", "value": "500 linhas", "keys": ["pedido"],
                       "replaces": 1, "waves": [1]}),
            ),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        let agreed = section_lines(&built[0].text, crate::platform::i18n::translate("prompt.part.agreed", Locale::PtBr));
        assert_eq!(agreed, ["`agreed`: MSTD-LIMIT-0001"], "{}", built[0].text);
        assert_eq!(built[0].text.matches("MSTD-LIMIT-0001").count(), 1, "{}", built[0].text);
        assert!(!built[0].text.contains("linhas."), "nenhum texto de item entra: {}", built[0].text);
    }

    /// O pedido de uma onda e o da revisão dela, montados como a rodada os
    /// monta — com a cópia que ela criou para a onda —, listam só os códigos:
    /// cada parte traz uma linha por bloco da spec, com os códigos em
    /// sequência (os da onda na ordem de execução que ela declara). O comando
    /// de leitura aparece uma vez só, no exemplo, com o caminho do
    /// repositório principal, e nenhum item repete o comando nem o código.
    #[test]
    fn the_wave_and_review_requests_list_only_the_codes_per_block_with_one_example() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let everywhere = json!({"files": ["**"]});
        let log = log_of(&[
            ("context", json!({"text": "O objetivo da obra.", "keys": ["objetivo"], "label": "objetivo"})),
            ("context", json!({"text": "Como reproduzir.", "keys": ["reproduzir"], "label": "reproduzir"})),
            ("decision", json!({"text": "Os códigos em sequência.", "keys": ["códigos"], "why": "menos linhas",
                                "applies_to": everywhere})),
            ("rule", json!({"text": "Um exemplo só.", "keys": ["exemplo"], "example": "e", "applies_to": everywhere})),
            ("criterion", json!({"when": "a rodada monta", "then": "a lista sai curta", "proof": "cargo test"})),
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [5], "done_when": "passa", "order": [8, 7]})),
            ("task", json!({"wave": 1, "text": "Montar a lista", "files": [{"path": "src/a.rs"}]})),
            ("task", json!({"wave": 1, "text": "Trocar o texto", "files": [{"path": "src/b.rs"}]})),
            ("delivered", json!({"wave": 1, "text": "A lista saiu.", "files": ["src/a.rs"]})),
        ]);
        let copy = WaveCopy { path: "/c/um".into(), build_dir: Some("/t/a".into()) };
        let flight = Flight { running: [1].into(), copies: [(1, copy)].into(), ..Flight::default() };
        let built = prompts(root, "teste", &log, Locale::PtBr, &flight);
        let part = |key: &str| crate::platform::i18n::translate(key, Locale::PtBr);
        let example = part("prompt.read")
            .replace("{root}", &format!("--root {} ", shown(root)))
            .replace("{spec}", "teste");
        let command = format!("`mustard-rt run read <bloco> --root {} --spec teste --term <código>`", shown(root));
        assert!(example.contains(&command), "{example}");
        let (wave, review) = (&built[0].text, &built[0].review);

        let waves = ["`waves`: MSTD-TASK-0002, MSTD-TASK-0001, MSTD-WAVE-0001"];
        let criteria = ["`criteria`: MSTD-CRIT-0001"];
        assert_eq!(section_lines(wave, part("prompt.part.specification")), ["`specification`: MSTD-CTX-0001, MSTD-CTX-0002"], "{wave}");
        assert_eq!(section_lines(wave, part("prompt.part.agreed")), ["`agreed`: MSTD-DEC-0001, MSTD-RULE-0001"], "{wave}");
        assert_eq!(section_lines(wave, part("prompt.part.wave")), waves, "{wave}");
        assert_eq!(section_lines(wave, part("prompt.part.criteria")), criteria, "{wave}");
        assert_eq!(section_lines(review, part("prompt.part.wave")), waves, "{review}");
        assert_eq!(section_lines(review, part("prompt.part.own_delivered")), ["`waves`: MSTD-DELIV-0001"], "{review}");
        assert_eq!(section_lines(review, part("prompt.part.criteria")), criteria, "{review}");

        for text in [wave, review] {
            assert!(text.contains(&example), "{text}");
            for once in ["mustard-rt run read", "--term", "--root", "MSTD-TASK-0001", "MSTD-WAVE-0001", "MSTD-CRIT-0001"] {
                assert_eq!(text.matches(once).count(), 1, "{once}: {text}");
            }
            for copied in ["O objetivo da obra.", "Montar a lista", "a lista sai curta", "A lista saiu."] {
                assert!(!text.contains(copied), "{copied} foi copiado: {text}");
            }
        }
    }

    /// O pedido do revisor de uma onda leva os defeitos já vistos nos
    /// arquivos dela; o pedido do agente da onda não os leva.
    #[test]
    fn the_reviewer_of_a_wave_gets_the_defects_already_seen_in_those_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let bank = root.join(".claude").join("spec");
        std::fs::create_dir_all(&bank).unwrap();
        std::fs::write(
            bank.join("lessons.ndjson"),
            [
                bank_line(1, "defect", "Apagar a pasta perde trabalho.", "apps/rt/src/**"),
                bank_line(2, "project_rule", "O comentário vai em português.", "apps/rt/src/**"),
            ]
            .concat(),
        )
        .unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Apagar a pasta velha e escrever o comentário.", "files": [{"path": "apps/rt/src/a.rs"}]})),
        ]);

        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        let heading = crate::platform::i18n::translate("prompt.part.defects", Locale::PtBr);
        let (before, defects) = built[0].review.split_once(heading).expect("a seção dos defeitos");
        assert!(defects.contains("Apagar a pasta perde trabalho."), "{defects}");
        assert!(!defects.contains("O comentário vai em português."), "só defeito entra: {defects}");
        assert!(!before.contains("Apagar a pasta"), "{before}");
        assert!(!built[0].text.contains(heading), "o pedido da onda não tem a seção: {}", built[0].text);
    }

    /// As linhas de uma seção do pedido, sem o "- " do começo; vazio quando
    /// a seção não está lá.
    fn section_lines<'a>(text: &'a str, heading: &str) -> Vec<&'a str> {
        let Some((_, after)) = text.split_once(&format!("## {heading}")) else { return Vec::new() };
        let section = after.split("\n## ").next().unwrap_or_default();
        section.lines().filter_map(|line| line.strip_prefix("- ")).collect()
    }

    /// Um banco com seis defeitos ligados às tarefas, dois sem palavra em
    /// comum com elas e uma preferência também sem: o pedido da onda e o da
    /// revisão levam só os cinco defeitos mais ligados. O sexto, que divide
    /// uma palavra só, perde o lugar; os sem palavra em comum ficam fora dos
    /// dois pedidos.
    #[test]
    fn the_wave_and_review_requests_carry_only_the_lessons_tied_to_the_tasks() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let folder = "apps/rt/src/**";
        let strong: Vec<String> =
            (1..=5).map(|n| format!("Somar o total da fatura errou o relatório {n}.")).collect();
        let weak = "A fatura antiga fica no arquivo morto.";
        let loose = ["Apagar a pasta perde trabalho.", "O gancho nunca entra em pânico."];
        let mut bank = String::new();
        let mut id = 0;
        let mut next = |class: &str, text: &str| {
            id += 1;
            bank.push_str(&bank_line(id, class, text, folder));
        };
        next("defect", loose[0]);
        for text in &strong {
            next("defect", text);
        }
        next("defect", weak);
        next("defect", loose[1]);
        next("user_preference", "Resposta curta.");
        let path = root.join(".claude").join("spec");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("lessons.ndjson"), bank).unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar o total da fatura no relatório.",
                            "files": [{"path": "apps/rt/src/a.rs"}]})),
        ]);

        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        let expected: Vec<&str> = strong.iter().map(String::as_str).collect();
        let lessons = section_lines(&built[0].text, crate::platform::i18n::translate("prompt.part.lessons", Locale::PtBr));
        assert_eq!(lessons, expected, "o pedido da onda: {}", built[0].text);
        let defects = section_lines(&built[0].review, crate::platform::i18n::translate("prompt.part.defects", Locale::PtBr));
        assert_eq!(defects, expected, "o pedido da revisão: {}", built[0].review);
        for out in [weak, loose[0], loose[1], "Resposta curta."] {
            assert!(!built[0].text.contains(out) && !built[0].review.contains(out), "{out} ficou de fora dos dois pedidos");
        }
    }

    /// Uma lição como o gravador a deixa no banco, com o `search`. Cada
    /// palavra do texto é também uma palavra-chave dela: é nas palavras-chave
    /// que o pedido procura as lições ligadas às tarefas.
    fn bank_line(id: u64, class: &str, text: &str, files: &str) -> String {
        let keys: Vec<&str> = text.trim_end_matches('.').split(' ').collect();
        keyed_line(id, class, text, &keys, json!({"files": [files]}), json!({"source": "apps/rt/CLAUDE.md"}))
    }

    /// Uma lição como o gravador a deixa no banco, com as palavras-chave, o
    /// lugar em que vale e o lugar em que nasceu dados.
    fn keyed_line(id: u64, class: &str, text: &str, keys: &[&str], applies_to: Value, found_in: Value) -> String {
        let draft = json!({"class": class, "text": text, "keys": keys, "applies_to": applies_to, "found_in": found_in});
        let event = crate::domain::lessons::normalize(draft.as_object().cloned().unwrap_or_default(), None);
        format!("{}\n", render_line(&stamp(event, id, None, "2026-09-15T10:00:00-03:00")))
    }

    /// O caso real: as quatro lições do projeto todo do banco deste projeto,
    /// com o texto e as palavras-chave de verdade, e a tarefa real da onda
    /// que só mexe na barra de status. A lição longa sobre o envio do pedido
    /// divide palavras do texto com a tarefa ("primeira", "linha"), mas
    /// nenhuma palavra-chave dela aparece na tarefa: ela fica fora do pedido
    /// da onda e do da revisão. A lição do teste, cuja palavra-chave "teste"
    /// aparece na tarefa, continua entrando nos dois; a da trava, cuja
    /// palavra-chave "ao mesmo tempo" só aparece pela metade ("tempo"), e a
    /// da suíte ficam fora.
    #[test]
    fn the_request_carries_a_lesson_by_its_keywords_and_not_by_the_words_of_its_text() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let everywhere = json!({"files": ["**"]});
        let lines = [
            keyed_line(1, "defect", DISPATCH_LESSON, &["pedido", "onda", "subagente", "despacho", "contexto"], everywhere.clone(), json!({"spec": "mustard-enxuto"})),
            keyed_line(3, "defect", "Subagente nunca roda compilação ou teste em segundo plano: o aviso de término não chega a ele, e ele encerra dizendo que espera a suíte, com o trabalho pela metade. Rode tudo em primeiro plano, com o teto de tempo do comando, e só devolva o relatório depois de ver o resultado. Em 17/09 duas revisões pararam assim e precisaram ser retomadas.", &["segundo plano", "suíte", "subagente", "revisão", "espera"], everywhere.clone(), json!({"spec": "mustard-enxuto"})),
            keyed_line(4, "defect", "Quem tira uma proteção (trava, reserva, recusa) prova que o que ela protegia continua protegido: a trava que sobra cobre o bloco inteiro (ler, juntar, gravar, comitar e desfazer), e o teste roda duas voltas ao mesmo tempo, não só uma de cada vez. Em 17/09 a onda 33 tirou a reserva de arquivos e a trava do git seguiu cobrindo só o commit: duas rodadas simultâneas no mesmo arquivo perdiam a mudança de uma delas para sempre.", &["trava", "reserva", "ao mesmo tempo", "concorrência", "proteção"], everywhere.clone(), json!({"spec": "mustard-enxuto"})),
            keyed_line(96, "defect", "O teste tem de falhar quando o código está errado. Teste na divisa: o último valor que passa e o primeiro que já não passa. Teste pelo caminho que a pessoa usa, com os ganchos e os comandos, sem montar a conversa à mão. E teste com os dados completos de uma sessão real. Em 18/09, três testes desta obra passavam com o código errado por falta disso.", &["teste", "prova", "vermelho", "divisa", "caminho real", "dados completos"], everywhere, json!({"spec": "conferencia-instalacao-barra"})),
        ];
        let path = root.join(".claude").join("spec");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("lessons.ndjson"), lines.concat()).unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 7, "text": "Os dois ajustes pedidos depois das revisões", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 7, "label": "Onda 7, a barra sem a contagem de linhas mudadas", "text": STATUS_BAR_TASK,
                            "files": [{"path": "apps/rt/src/commands/statusline/mod.rs"}, {"path": "apps/rt/src/commands/statusline/segment.rs"}]})),
        ]);

        let bank = crate::io::lessons::read(&path.join("lessons.ndjson")).unwrap().unwrap();
        let by_text = crate::domain::lessons::matching(&bank, STATUS_BAR_TASK);
        assert!(by_text.iter().any(|hit| hit.id == 1), "pelo texto inteiro, a lição do envio entraria: {by_text:?}");

        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        let part = |key: &str| crate::platform::i18n::translate(key, Locale::PtBr);
        let lessons = section_lines(&built[0].text, part("prompt.part.lessons"));
        let defects = section_lines(&built[0].review, part("prompt.part.defects"));
        for shown in [&lessons, &defects] {
            assert_eq!(shown.len(), 1, "{shown:?}");
            assert!(shown[0].starts_with("O teste tem de falhar"), "{shown:?}");
        }
        for out in ["O pedido da onda vai INTEIRO", "Subagente nunca roda", "Quem tira uma proteção"] {
            assert!(!built[0].text.contains(out) && !built[0].review.contains(out), "{out} fica fora dos dois pedidos");
        }
    }

    /// A lição real sobre o envio do pedido de uma onda, do banco deste
    /// projeto.
    const DISPATCH_LESSON: &str = "O pedido da onda vai INTEIRO no corpo do prompt do subagente, como a rodada o devolve. Mandar no lugar dele um endereco — o codigo do envio gravado, ou um arquivo temporario — nao economiza contexto nenhum: a primeira coisa que o agente faz e abrir esse endereco e receber o mesmo texto de uma vez, so que em JSON cru e gastando uma chamada a mais. O que e preguicoso nao e o pedido, sao os itens dentro dele: cada linha da lista e um endereco, e o agente so abre o conteudo de um item quando chega a hora de trabalhar nele. Em 16/09 o usuario cortou dois agentes seguidos por isso, um por arquivo temporario e outro pelo codigo do envio.";

    /// A tarefa real da onda que só mexe na barra de status.
    const STATUS_BAR_TASK: &str = "A primeira linha da barra deixa de mostrar a contagem de linhas mudadas (+N-N) e fica igual ao exemplo aprovado: projeto, branch, spec, uso da conversa e tempo. O teste do exemplo aprovado passa a usar uma sessão com linhas mudadas e todos os dados que a sessão real traz, e reprova se a contagem voltar.";

    /// Uma pasta com 500 regras do projeto, e a onda mexe num arquivo dela: o
    /// pedido leva no máximo 5 regras, as mais ligadas ao texto das tarefas,
    /// e fica abaixo do teto de linhas. A regra que só divide uma palavra com
    /// a tarefa perde o lugar para as que dividem quatro; das outras classes
    /// entra só a lição ligada à tarefa.
    #[test]
    fn a_folder_with_hundreds_of_rules_sends_at_most_the_five_closest_to_the_tasks() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let folder = "apps/rt/src/**";
        let mut bank = String::new();
        let mut id = 0;
        let mut next = |class: &str, text: String| {
            id += 1;
            bank.push_str(&bank_line(id, class, &text, folder));
        };
        for n in 1..=500 {
            next("project_rule", format!("Regra {n}: cada módulo declara o dono dele."));
        }
        let strong: Vec<String> =
            (1..=5).map(|n| format!("Somar o total da fatura exige conferir o relatório {n}.")).collect();
        let weak: Vec<String> = (1..=2).map(|n| format!("A fatura antiga fica no arquivo morto {n}.")).collect();
        for text in strong.iter().chain(&weak) {
            next("project_rule", text.clone());
        }
        next("defect", "Apagar a pasta perde trabalho.".to_string());
        next("user_preference", "O total da fatura sai em reais.".to_string());
        let path = root.join(".claude").join("spec");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("lessons.ndjson"), bank).unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar o total da fatura no relatório.",
                            "files": [{"path": "apps/rt/src/a.rs"}]})),
        ]);

        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        assert!(built[0].too_long.is_none(), "{} linhas", built[0].lines);
        assert!(built[0].lines <= wave_prompt::MAX_LINES, "{} linhas", built[0].lines);
        let shown = section_lines(&built[0].text, crate::platform::i18n::translate("prompt.part.lessons", Locale::PtBr));
        let rules: Vec<&str> = shown.iter().copied().filter(|line| line.contains("fatura") || line.contains("Regra")).filter(|line| !line.contains("reais")).collect();
        assert_eq!(rules, strong.iter().map(String::as_str).collect::<Vec<_>>(), "{shown:?}");
        assert!(!shown.contains(&"Apagar a pasta perde trabalho."), "o defeito sem palavra em comum sai: {shown:?}");
        assert!(shown.contains(&"O total da fatura sai em reais."), "{shown:?}");
        assert_eq!(shown.len(), 6, "{shown:?}");
    }

    /// A skill cujo arquivo de exemplo mudou no git depois dela sai marcada
    /// como a revisar; a que não mudou não sai marcada.
    #[test]
    fn a_skill_whose_example_changed_after_it_is_marked_for_review() {
        let written = chrono::DateTime::parse_from_rfc3339("2026-09-15T10:00:00-03:00").unwrap().timestamp();
        for (moved, marked) in [(written + 3_600, true), (written - 3_600, false)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            write_skill(root, "apps/rt", "somar", "# O molde\n\nUm passo por linha.\n");
            std::fs::create_dir_all(root.join(".claude")).unwrap();
            std::fs::write(
                crate::io::project_map::model_path(root),
                json!({
                    "history": {
                        "paths": ["apps/rt/src/exemplo.rs"],
                        "commits": [{"id": "abc1234", "at": moved, "changed": [0]}],
                    }
                })
                .to_string(),
            )
            .unwrap();
            let mut events = vec![
                ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
                (
                    "task",
                    json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}], "skill": "somar"}),
                ),
            ];
            events.push((
                "skill",
                json!({"name": "somar", "action": "create", "text": "O molde", "sha": "3f9a1c2e",
                       "examples": [{"path": "apps/rt/src/exemplo.rs", "why": "mesma pasta"}]}),
            ));
            let built = prompts(root, "teste", &log_of(&events), Locale::PtBr, &Flight::default());
            assert!(built[0].bad_skills.is_empty(), "{:?}", built[0].bad_skills);
            assert_eq!(built[0].stale_skills.is_empty(), !marked, "mudou em {moved}");
            let review = crate::platform::i18n::translate("prompt.skill.stale", Locale::PtBr);
            assert_eq!(built[0].text.contains(&format!("**somar** ({review})")), marked);
            assert!(built[0].text.contains("skills/somar/SKILL.md"), "{}", built[0].text);
        }
    }

    /// O pedido da onda que sai agora traz a cópia que a rodada escolheu; o da
    /// onda em andamento, a cópia gravada no envio dela, e o da onda que não
    /// está fora não fala de cópia. O pedido do revisor traz a cópia dele,
    /// dentro das cópias do checkout, e a pasta de compilação que a cópia da
    /// onda usou. O projeto é Rust: o mapa marca a raiz como `cargo`.
    #[test]
    fn the_request_carries_the_copy_the_round_chose_or_recorded_and_the_review_its_build_folder() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        let model = json!({"projects": [{"name": "(root)", "dir": "", "kind": "cargo"}]}).to_string();
        std::fs::write(crate::io::project_map::model_path(root), model).unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "src/a.rs"}]})),
            ("wave", json!({"n": 2, "text": "Outra onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 2, "text": "Subtrair", "files": [{"path": "src/a.rs"}]})),
            (
                "send",
                json!({"wave": 1, "role": "wave", "text": "p", "lines": 1, "chars": 1, "items": [1], "mustard": "0",
                       "author": "binary", "copy": "/c/um", "build_dir": "/t/a"}),
            ),
        ]);
        let chosen = WaveCopy { path: "/c/dois".into(), build_dir: Some("/t/b".into()) };
        let flight = Flight { running: [1, 2].into(), copies: [(2, chosen)].into(), ..Flight::default() };
        let built = prompts(root, "teste", &log, Locale::PtBr, &flight);
        let rule = |key: &str, from: &str, to: &str| crate::platform::i18n::translate(key, Locale::PtBr).replace(from, to);
        assert!(built[0].text.contains(&rule("prompt.execution.build_dir", "{dir}", "/t/a")), "{}", built[0].text);
        assert!(built[0].text.contains("`/c/um`"), "{}", built[0].text);
        assert!(built[1].text.contains("`/c/dois`") && built[1].text.contains("CARGO_TARGET_DIR=/t/b"), "{}", built[1].text);

        let review = shown(&copy_path(root, "teste", 1, true));
        assert!(review.ends_with("/.claude/worktrees/mustard-teste-1-review"), "{review}");
        assert!(built[0].review.contains(&format!("--detach {review} HEAD`")), "{}", built[0].review);
        assert!(built[0].review.contains("CARGO_TARGET_DIR=/t/a`"), "{}", built[0].review);
        assert!(built[0].review.contains(&format!("--root {} --spec teste", shown(root))), "{}", built[0].review);
        assert!(built[0].text.contains(&format!("--root {} --spec teste", shown(root))), "{}", built[0].text);

        let still = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        assert!(!still[0].text.contains("/c/um") && !still[0].text.contains("CARGO_TARGET_DIR"), "{}", still[0].text);
        assert!(!still[0].text.contains("--root"), "sem cópia, o agente lê a spec de onde está: {}", still[0].text);
    }

    /// Os pedidos que a rodada monta — o da onda, o do revisor dela e o da
    /// revisão final — num projeto sem mapa, num só com parte Node, num com
    /// uma parte Node e uma Rust e num só Rust. A onda tem a cópia e a pasta
    /// de compilação que a rodada escolheu nos quatro, mas só os dois com
    /// parte `cargo` no mapa trazem a frase da pasta e citam o Cargo e a
    /// pasta `target/copias`; a cópia aparece em todos.
    #[test]
    fn the_build_folder_rule_goes_only_to_rust_projects() {
        let node = json!({"name": "web", "dir": "web", "kind": "npm", "code_files": 3});
        let rust = json!({"name": "api", "dir": "api", "kind": "cargo", "code_files": 3});
        for (map, cites) in [
            (None, false),
            (Some(json!([node])), false),
            (Some(json!([node, rust])), true),
            (Some(json!([rust])), true),
        ] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            if let Some(projects) = &map {
                std::fs::create_dir_all(root.join(".claude")).unwrap();
                let model = json!({"projects": projects}).to_string();
                std::fs::write(crate::io::project_map::model_path(root), model).unwrap();
            }
            let folder = shown(&root.join("target").join("copias").join("a"));
            let copy = shown(&copy_path(root, "teste", 1, false));
            let log = log_of(&[
                ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
                ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "src/a.rs"}]})),
                (
                    "send",
                    json!({"wave": 1, "role": "wave", "text": "p", "lines": 1, "chars": 1, "items": [1],
                           "mustard": "0", "author": "binary", "copy": copy, "build_dir": folder}),
                ),
            ]);
            let flight = Flight { running: [1].into(), ..Flight::default() };
            for lang in [Locale::PtBr, Locale::EnUs] {
                let built = prompts(root, "teste", &log, lang, &flight);
                let last = final_review(root, "teste", &log, lang);
                let sentence = crate::platform::i18n::translate("prompt.execution.build_dir", lang).replace("{dir}", &folder);
                for (what, text) in [("wave", &built[0].text), ("review", &built[0].review), ("final", &last)] {
                    assert_eq!(text.contains(&sentence), cites, "{map:?} {lang:?} {what}: {text}");
                    assert_eq!(text.contains("Cargo"), cites, "{map:?} {lang:?} {what}: {text}");
                    assert_eq!(text.contains("target/copias"), cites, "{map:?} {lang:?} {what}: {text}");
                }
                assert!(built[0].text.contains(&format!("`{copy}`")), "{map:?} {lang:?}: {}", built[0].text);
            }
        }
    }

    /// A skill que a tarefa nomeia e que não está no disco é recusada, com o
    /// caminho em que ela devia estar.
    #[test]
    fn a_named_skill_that_is_not_on_disk_is_refused_with_the_path() {
        let dir = tempdir().unwrap();
        let built = prompts(dir.path(), "teste", &plan_log(), Locale::PtBr, &Flight::default());
        assert_eq!(built[0].bad_skills.len(), 1);
        assert_eq!(built[0].bad_skills[0].1.reason(), "skill-missing-path");
        assert!(built[0].bad_skills[0].1.message(Locale::PtBr).contains("somar"));
    }

    /// Com a spec real desta obra: nenhum item combinado fica sem dono, e o
    /// pedido de cada onda, montado como a rodada o monta, só cita item
    /// combinado dela ou do projeto. A spec fica fora do git, então o teste
    /// roda à mão (`--ignored`); `MUSTARD_SPEC_FILE` aponta outra cópia dela.
    #[test]
    #[ignore = "lê a spec real, que fica fora do git"]
    fn with_the_real_spec_every_agreed_item_has_an_owner_and_each_request_cites_only_its_own() {
        use crate::domain::mustard_id;
        use crate::domain::wave_prompt::Owner;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let file = std::env::var_os("MUSTARD_SPEC_FILE")
            .map_or_else(|| root.join(".claude/spec/mustard-enxuto/spec.ndjson"), PathBuf::from);
        let log = crate::io::spec_events::read(&file).expect("a spec se lê").expect("a spec existe");
        let spec = file.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().to_string()).unwrap();
        let codes = log.codes();
        let unowned: Vec<&String> = wave_prompt::unowned(&log).iter().filter_map(|e| codes.get(&e.id)).collect();
        assert!(unowned.is_empty(), "{} itens combinados sem dono: {unowned:?}", unowned.len());

        let owners = wave_prompt::owners(&log);
        let by_code: std::collections::BTreeMap<&str, u64> =
            codes.iter().map(|(id, code)| (code.as_str(), *id)).collect();
        let built = prompts(&root, &spec, &log, Locale::PtBr, &Flight::default());
        assert_eq!(built.len(), log.planned_waves().len());
        for prompt in &built {
            for (start, end) in mustard_id::find(&prompt.text) {
                let code = &prompt.text[start..end];
                let Some(item) = by_code.get(code).and_then(|id| log.get(*id)) else { continue };
                if item.block() != Some(Block::Agreed) {
                    continue;
                }
                match owners.get(&item.id) {
                    Some(Owner::Project) => {}
                    Some(Owner::Waves(waves)) if waves.contains(&prompt.wave) => {}
                    other => panic!("o pedido da onda {} cita {code}, que é de {other:?}", prompt.wave),
                }
            }
        }
    }
}
