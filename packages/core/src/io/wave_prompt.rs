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

use crate::domain::lessons::{in_scope, related_to_tasks, Scope};
use crate::domain::project_map::{check_skill, file_history, has_rust_part, tests_for, MapRefusal, ProjectMap};
use crate::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog};
use crate::domain::wave_prompt::{self, wave_files, Choice, Execution, Material, Skill, WaveCopy};
use crate::platform::i18n::Locale;

/// O pedido de uma onda, como o disco o entrega.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavePrompt {
    /// O número da onda.
    pub wave: u64,
    /// O texto do molde do agente, como o instalador o gravou no projeto —
    /// o primeiro dos dois textos que o agente recebe. Vazio quando o
    /// projeto ainda não tem o arquivo do molde.
    pub template: String,
    /// O nome do agente escolhido para este lote, pelo número de tarefas
    /// ([`wave_prompt::agent_role`]): `"wave-solo"` numa tarefa só, `"wave"`
    /// em várias. É o arquivo lido para `template`, sem a extensão.
    pub agent: String,
    /// O modelo pedido para a onda: fixo, pelo papel `wave`
    /// ([`wave_prompt::requested_model`]).
    pub model: String,
    /// O texto do pedido, sempre: a página mostra mesmo o pedido recusado,
    /// que é justamente o que precisa ser visto antes da aprovação.
    pub text: String,
    /// Quantas linhas ele tem.
    pub lines: usize,
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
    /// A escolha do orquestrador antes do envio de cada onda que sai agora.
    /// A de uma onda que já saiu vem do envio gravado dela.
    pub choices: BTreeMap<u64, Choice>,
}

/// Os pedidos de todas as ondas do plano, em ordem de número, com as ondas
/// que estão fora em `flight`.
#[must_use]
pub fn prompts(root: &Path, spec: &str, log: &SpecLog, lang: Locale, flight: &Flight) -> Vec<WavePrompt> {
    let bank = lesson_bank(root);
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

/// O banco de lições do projeto `root`; `None` quando ele não existe ou não
/// se lê.
#[must_use]
pub fn lesson_bank(root: &Path) -> Option<SpecLog> {
    let paths = crate::ClaudePaths::for_project(root).ok()?;
    crate::io::lessons::read(&paths.lessons_path()).ok().flatten()
}

/// As lições do banco que casam com a onda `wave`: as que valem para os
/// arquivos das tarefas dela ou para as skills que elas nomeiam e, de cada
/// classe, só as mais ligadas ao texto das tarefas. Uma pasta com centenas
/// delas passaria do teto de linhas, e a lição sem palavra em comum com a
/// tarefa só ocupa o agente. São as que a rodada mostra ao orquestrador antes
/// do envio, e as que o pedido leva, menos as que a escolha dele tirou.
#[must_use]
pub fn wave_lessons<'a>(bank: &'a SpecLog, log: &SpecLog, wave: u64) -> Vec<&'a SpecEvent> {
    let files = wave_files(log, wave);
    let mut found: Vec<&SpecEvent> = Vec::new();
    for skill in std::iter::once(None).chain(skills_named(log, wave).into_iter().map(Some)) {
        let scope = Scope { files: files.clone(), subproject: None, skill };
        for lesson in in_scope(bank, &scope) {
            if !found.iter().any(|seen| seen.id == lesson.id) {
                found.push(lesson);
            }
        }
    }
    related_to_tasks(found, &tasks_text(log, wave))
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

/// O pedido do agente de teste dedicado da spec `spec`, que o fechamento pede
/// a toda obra, mesmo a de uma onda só, no lugar da revisão de cada onda: as
/// ondas do plano com as tarefas, as emendas gravadas para elas, a entrega
/// mais nova de cada onda, os critérios, os commits que já entraram na branch
/// e a cópia do revisor, no commit mais novo da spec e na pasta de compilação
/// que a última onda enviada usou.
///
/// Uma onda cuja última revisão reprovou e que já entregou o conserto (o
/// fechamento só chega aqui depois disso: veja
/// [`crate::domain::spec_events::SpecLog::last_rejected`]) restringe as
/// ondas, as emendas e as entregas a ela: o agente confere só o conserto, sem
/// reabrir a obra inteira.
#[must_use]
pub fn final_review(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> String {
    let planned = log.planned_waves();
    let fixing: BTreeSet<u64> = log.last_rejected().into_keys().filter(|n| planned.contains(n)).collect();
    let scope: &BTreeSet<u64> = if fixing.is_empty() { &planned } else { &fixing };
    let visible = log.block(BlockQuery::Block(Block::Waves));
    let block: Vec<&SpecEvent> = visible
        .iter()
        .copied()
        .filter(|e| matches!(e.event_type.as_str(), "wave" | "task") && e.wave().is_some_and(|n| scope.contains(&n)))
        .collect();
    let newest = log.last_by_wave("delivered");
    let own_delivered: Vec<&SpecEvent> = visible
        .iter()
        .copied()
        .filter(|e| e.event_type == "delivered")
        .filter(|e| e.wave().filter(|n| scope.contains(n)).and_then(|n| newest.get(&n)) == Some(&e.id))
        .collect();
    let criteria: Vec<&SpecEvent> =
        log.block(BlockQuery::Block(Block::Criteria)).into_iter().filter(|e| e.event_type == "criterion").collect();
    // Todo o combinado vigente, dono ou não de onda: a revisão final responde
    // por ele inteiro, mesmo numa rodada de conserto de uma onda só.
    let mut agreed: Vec<&SpecEvent> = wave_prompt::all_agreed(log);
    agreed.sort_by_key(|e| e.id);
    let changes: Vec<&SpecEvent> =
        log.block(BlockQuery::Block(Block::Progress)).into_iter().filter(|e| e.event_type == "commit").collect();
    let fix: Vec<&SpecEvent> = if fixing.is_empty() {
        Vec::new()
    } else {
        let verdicts = log.verdicts_by_wave();
        fixing.iter().filter_map(|n| verdicts.get(n).and_then(|v| v.last().copied())).collect()
    };
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
        agreed,
        fix,
        own_delivered,
        changes,
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
/// Um caminho como o pedido e o envio gravado o mostram: sempre com barras
/// normais, que o terminal e o controle de versão aceitam nos três sistemas.
#[must_use]
pub fn shown(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// O texto do molde do agente instalado no projeto `root`, para o papel
/// `role` (`wave`, `wave-solo`, `review` ou `skill`) — o arquivo que o
/// instalador grava em `.claude/agents/mustard/<role>.md`. Vazio quando o
/// projeto ainda não o tem.
#[must_use]
pub fn agent_template(root: &Path, role: &str) -> String {
    std::fs::read_to_string(root.join(".claude").join("agents").join("mustard").join(format!("{role}.md")))
        .unwrap_or_default()
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
    // O que a montagem escolhe, com a escolha do orquestrador antes do envio:
    // a que a rodada traz agora ou a gravada no envio da onda.
    let fresh = context.flight.choices.get(&wave);
    let read = wave_prompt::dispatch_items(log, wave, fresh);
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
    let choice = wave_prompt::choice_for(log, wave, fresh);
    // As skills que as tarefas nomeiam de saída e as que a escolha antes do
    // envio confirmou, juntas, sem repetir.
    let mut named = skills_named(log, wave);
    for chosen in choice.as_ref().into_iter().flat_map(|c| c.tasks.iter()).flat_map(|t| t.skills.iter()) {
        if !named.contains(chosen) {
            named.push(chosen.clone());
        }
    }
    named.sort();
    // Os arquivos de leitura de cada tarefa: os que ela já declara em
    // `must_read` — obrigatórios, sem passar pela escolha do orquestrador,
    // como os itens que a onda já faz — e os que a escolha antes do envio
    // confirmou. Um caminho que não existe no projeto (a parte antes do `#`,
    // quando ele aponta uma função) fica de fora; a tarefa sem nenhum arquivo
    // não entra.
    let codes = log.codes();
    let mut task_reads: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for task in of_type("task") {
        let must_read = task
            .fields
            .get("must_read")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
            .filter(|file| cited_exists(root, file));
        task_reads.entry(task.id).or_default().extend(must_read.map(str::to_string));
    }
    for chosen in choice.as_ref().into_iter().flat_map(|c| c.tasks.iter()).filter(|t| !t.files.is_empty()) {
        task_reads.entry(chosen.task).or_default().extend(chosen.files.iter().cloned());
    }
    let task_reads: Vec<(String, Vec<String>)> = task_reads
        .into_iter()
        .filter(|(_, files)| !files.is_empty())
        .map(|(task, mut files)| {
            files.sort();
            files.dedup();
            let files = files.into_iter().map(|file| with_current_lines(map, file)).collect();
            (codes.get(&task).cloned().unwrap_or_else(|| task.to_string()), files)
        })
        .collect();
    // Os arquivos de teste que o mapa do projeto conhece para cada arquivo
    // que uma tarefa cita: a linha da tarefa os lista logo abaixo do
    // arquivo, para o agente não sair procurando um por um no código. Um
    // arquivo que o mapa não conhece, ou sem teste externo conhecido, fica
    // de fora — a linha continua como hoje, sem inventar nada.
    let file_tests = task_file_tests(map, &of_type("task"));
    // As lições que casam com a onda, menos as que a escolha do orquestrador
    // tirou. O pedido da revisão leva as mesmas.
    let lessons: Vec<&SpecEvent> = bank
        .map(|bank| wave_lessons(bank, log, wave))
        .unwrap_or_default()
        .into_iter()
        .filter(|lesson| !choice.as_ref().is_some_and(|choice| choice.removes_lesson(lesson.id)))
        .collect();

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
        skills,
        task_reads,
        file_tests,
        changes: Vec::new(),
        codes,
    };
    let text = wave_prompt::write(&material, lang);
    let lines = wave_prompt::count_lines(&text);
    // O nome do agente, pelo número de tarefas do lote, escolhe o arquivo:
    // nenhum dos dois limita as idas e voltas do agente, então não há mais o
    // que escrever em memória — só ler o molde certo.
    let agent = wave_prompt::agent_role(of_type("task").len()).to_string();
    let template = agent_template(root, &agent);
    let model = wave_prompt::requested_model(&agent).to_string();
    WavePrompt { wave, template, agent, model, text, lines, bad_skills, stale_skills }
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

/// Um caminho citado (pela skill, ou pela leitura obrigatória de uma tarefa)
/// existe? A conferência olha só a parte antes do `#`: a leitura obrigatória
/// aceita `caminho#função`, e a função não é um arquivo. Pode citar só o fim
/// do caminho, então a busca é pelo que o projeto tem.
fn cited_exists(root: &Path, cited: &str) -> bool {
    let cited = cited.split('#').next().unwrap_or(cited);
    if root.join(cited).exists() {
        return true;
    }
    crate::io::project_map::read(root)
        .is_ok_and(|map| map.modules.iter().any(|m| m.path.ends_with(cited)))
}

/// Um arquivo de leitura obrigatória `caminho#função`, com as linhas atuais
/// da declaração anexadas ao fim (`@início-fim`, uma faixa por trecho,
/// separadas por vírgula, para o nome que se repete no arquivo), achadas no
/// mapa do projeto pelo caminho do módulo e pelo nome da declaração. Sem
/// mapa, sem a função nele ou sem a linha final dela, o arquivo volta sem
/// mudança: [`wave_prompt::WavePrompt::text`] então só manda ler a função
/// pelo nome, como antes. Um caminho sozinho (sem `#`) também volta sem
/// mudança.
fn with_current_lines(map: Option<&ProjectMap>, file: String) -> String {
    let Some((path, name)) = file.split_once('#') else { return file };
    if path.is_empty() || name.is_empty() {
        return file;
    }
    let Some(ranges) = decl_lines(map, path, name) else { return file };
    let lines = ranges.iter().map(|(start, end)| format!("{start}-{end}")).collect::<Vec<_>>().join(", ");
    format!("{file}@{lines}")
}

/// Os arquivos de teste que o mapa do projeto conhece para cada arquivo
/// citado pelas tarefas `tasks`, pelo caminho como a tarefa o escreve. Sem
/// mapa, ou para um arquivo que ele não conhece ou sem teste externo
/// conhecido, o arquivo fica de fora.
fn task_file_tests(map: Option<&ProjectMap>, tasks: &[&SpecEvent]) -> BTreeMap<String, Vec<String>> {
    let Some(map) = map else { return BTreeMap::new() };
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for task in tasks {
        let paths = task.fields.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        for path in paths.iter().filter_map(|file| file.get("path").and_then(Value::as_str)) {
            if out.contains_key(path) {
                continue;
            }
            if let Ok(coverage) = tests_for(map, path)
                && !coverage.files.is_empty()
            {
                out.insert(path.to_string(), coverage.files);
            }
        }
    }
    out
}

/// As faixas de linha (começo, fim) de cada declaração de nome `name` no
/// módulo `path` — mais de uma quando o nome se repete no arquivo. `None`
/// quando o mapa não tem o módulo, ou quando nenhuma ocorrência tem a linha
/// final resolvida.
fn decl_lines(map: Option<&ProjectMap>, path: &str, name: &str) -> Option<Vec<(u64, u64)>> {
    let module = map?.modules.iter().find(|m| m.path == path || m.path.ends_with(path))?;
    let ranges: Vec<(u64, u64)> =
        module.declarations.iter().filter(|d| d.name == name && d.end_line > 0).map(|d| (d.line, d.end_line)).collect();
    (!ranges.is_empty()).then_some(ranges)
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

    /// Uma onda `n` com `tasks` tarefas, para testar o molde de agente que
    /// o número delas escolhe.
    fn plan_log_with_tasks(tasks: usize) -> SpecLog {
        let mut events: Vec<(&str, Value)> =
            vec![("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"}))];
        for _ in 0..tasks {
            events.push(("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}]})));
        }
        log_of(&events)
    }

    /// Os moldes de agente de verdade, os que o Mustard instala no projeto,
    /// gravados em `root` como o instalador os grava. É o molde do produto,
    /// e não uma cópia de mentira escrita aqui, que o teste lê: assim um
    /// teto de idas e voltas que voltasse ao cabeçalho derrubaria o teste.
    fn write_agent_template(root: &Path) {
        let dir = root.join(".claude").join("agents").join("mustard");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in crate::platform::seeds::agent_texts(Locale::PtBr) {
            std::fs::write(dir.join(format!("{name}.md")), body).unwrap();
        }
    }

    fn write_skill(root: &Path, subproject: &str, name: &str, text: &str) {
        let dir = root.join(subproject).join(".claude").join("skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), text).unwrap();
    }

    /// O mapa de teste, gravado como o scan o grava: cria a pasta `.claude`
    /// antes de escrever o arquivo do mapa.
    fn write_map(root: &Path, model: &Value) {
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(crate::io::project_map::model_path(root), model.to_string()).unwrap();
    }

    /// O molde que o pedido da onda leva não traz teto de idas e voltas,
    /// nem o de tarefa única (`wave-solo.md`) nem o de várias (`wave.md`).
    /// A medição de treze agentes de onda deste projeto deu de 36 a 315
    /// idas e voltas, e a onda mais curta gastou 36: qualquer teto cortava
    /// a onda no meio e a fazia recomeçar do zero, gastando mais do que se
    /// não houvesse teto. O binário só escolhe qual dos dois moldes ler.
    #[test]
    fn o_molde_do_agente_de_onda_nao_traz_teto_de_idas_e_voltas() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_agent_template(root);

        let solo = prompts(root, "teste", &plan_log_with_tasks(1), Locale::PtBr, &Flight::default());
        assert_eq!(solo.len(), 1);
        assert_eq!(solo[0].agent, "wave-solo", "{}", solo[0].agent);
        assert!(!solo[0].template.contains("maxTurns"), "{}", solo[0].template);

        let several = prompts(root, "teste", &plan_log_with_tasks(2), Locale::PtBr, &Flight::default());
        assert_eq!(several.len(), 1);
        assert_eq!(several[0].agent, "wave", "{}", several[0].agent);
        assert!(!several[0].template.contains("maxTurns"), "{}", several[0].template);
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
        let items = section_lines(&built[0].text, crate::platform::i18n::translate("prompt.part.items", Locale::PtBr));
        assert!(items.contains(&"`agreed`: MSTD-LIMIT-0001"), "{}", built[0].text);
        assert_eq!(built[0].text.matches("MSTD-LIMIT-0001").count(), 1, "{}", built[0].text);
        assert!(!built[0].text.contains("linhas."), "nenhum texto de item entra: {}", built[0].text);
    }

    /// O arquivo que uma tarefa cita, e cujos testes o mapa do projeto
    /// conhece, ganha logo abaixo da linha da tarefa quem o testa — o
    /// agente não sai procurando um por um no código. O outro arquivo da
    /// mesma tarefa, sem teste conhecido no mapa, não ganha linha nenhuma.
    #[test]
    fn the_wave_request_lists_the_tests_the_map_knows_for_each_task_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let model = json!({
            "modules": [
                {"path": "apps/rt/src/a.rs", "tests": ["apps/rt/tests/a_test.rs"]},
                {"path": "apps/rt/src/b.rs", "tests": []},
            ]
        });
        write_map(root, &model);
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar",
                            "files": [{"path": "apps/rt/src/a.rs"}, {"path": "apps/rt/src/b.rs"}]})),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        assert!(
            built[0].text.contains("  - quem testa `apps/rt/src/a.rs`: `apps/rt/tests/a_test.rs`"),
            "{}",
            built[0].text
        );
        assert!(!built[0].text.contains("quem testa `apps/rt/src/b.rs`"), "{}", built[0].text);
    }

    /// A leitura obrigatória de uma tarefa que aponta uma função
    /// (`caminho#função`) faz a linha da tarefa dizer o que ler antes, e
    /// manda ler só aquela função, não o arquivo inteiro. As frases de ler
    /// por trecho, rodar só os testes do que mudou e a suíte inteira uma vez
    /// no fim, em primeiro plano, saíram do catálogo — moraram para o molde
    /// do agente — e não voltam a aparecer no pedido.
    #[test]
    fn the_wave_request_asks_to_read_by_excerpt() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("apps/rt/src")).unwrap();
        std::fs::write(root.join("apps/rt/src/a.rs"), "fn soma() {}\n").unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}],
                            "must_read": ["apps/rt/src/a.rs#soma"]})),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        assert!(built[0].text.contains("leia antes: leia só `soma` em `apps/rt/src/a.rs`"), "{}", built[0].text);
        assert!(!built[0].text.contains("`apps/rt/src/a.rs#soma`"), "{}", built[0].text);
        for phrase in ["Leia por trecho", "só os testes do que mudou", "A suíte inteira roda uma vez no fim, em primeiro plano"] {
            assert!(!built[0].text.contains(phrase), "{phrase}: {}", built[0].text);
        }
    }

    /// A leitura obrigatória de uma tarefa que aponta uma função
    /// (`caminho#função`) que o mapa do projeto conhece manda ler só as
    /// linhas atuais dela, do começo ao fim — sem número que envelhece no
    /// plano. Depois de um commit que desloca a função (linhas somadas
    /// acima dela, entre uma montagem e a seguinte), o pedido novo traz as
    /// linhas novas, lidas de novo do mapa a cada montagem.
    #[test]
    fn the_wave_request_reads_the_current_lines_of_each_function() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("apps/rt/src")).unwrap();
        std::fs::write(root.join("apps/rt/src/a.rs"), "fn antes() {}\n\nfn soma() {\n    1 + 1;\n}\n").unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}],
                            "must_read": ["apps/rt/src/a.rs#soma"]})),
        ]);
        let write_model = |line: u64, end_line: u64| {
            let model = json!({
                "modules": [{
                    "path": "apps/rt/src/a.rs",
                    "declarations": [{"kind": "function", "name": "soma", "line": line, "end_line": end_line}],
                }]
            });
            write_map(root, &model);
        };

        write_model(3, 5);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        assert!(
            built[0].text.contains("leia só as linhas 3-5 de `soma` em `apps/rt/src/a.rs`"),
            "{}",
            built[0].text
        );
        assert!(
            !built[0].text.contains("leia só `soma` em `apps/rt/src/a.rs`"),
            "o texto sem linha não deve sobrar: {}",
            built[0].text
        );

        // Um commit soma dez linhas antes da função: ela se desloca, e o
        // mapa gravou as posições novas. O pedido seguinte traz as linhas
        // novas, lidas de novo do disco — não as da montagem anterior.
        write_model(13, 15);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        assert!(
            built[0].text.contains("leia só as linhas 13-15 de `soma` em `apps/rt/src/a.rs`"),
            "{}",
            built[0].text
        );
        assert!(!built[0].text.contains("3-5"), "{}", built[0].text);
    }

    /// A leitura obrigatória de uma tarefa nunca chama de função uma
    /// declaração que não é — uma estrutura ou uma constante ganham a mesma
    /// frase genérica que uma função, em português e em inglês.
    #[test]
    fn the_request_never_calls_a_declaration_a_function() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("apps/rt/src")).unwrap();
        std::fs::write(root.join("apps/rt/src/a.rs"), "struct Coisa;\n").unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Ajustar", "files": [{"path": "apps/rt/src/a.rs"}],
                            "must_read": ["apps/rt/src/a.rs#Coisa"]})),
        ]);
        let write_model = || {
            let model = json!({
                "modules": [{
                    "path": "apps/rt/src/a.rs",
                    "declarations": [{"kind": "struct", "name": "Coisa", "line": 1, "end_line": 1}],
                }]
            });
            write_map(root, &model);
        };
        write_model();

        let pt = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        let pt_line = pt[0].text.lines().find(|l| l.contains("Coisa")).unwrap_or_default();
        assert!(pt_line.ends_with("leia só as linhas 1-1 de `Coisa` em `apps/rt/src/a.rs`"), "{pt_line}");
        assert!(!pt_line.contains("função"), "{pt_line}");

        let en = prompts(root, "teste", &log, Locale::EnUs, &Flight::default());
        let en_line = en[0].text.lines().find(|l| l.contains("Coisa")).unwrap_or_default();
        assert!(en_line.ends_with("read only lines 1-1 of `Coisa` in `apps/rt/src/a.rs`"), "{en_line}");
        assert!(!en_line.contains("function"), "{en_line}");
    }

    /// A leitura obrigatória de uma tarefa que aponta um arquivo que não
    /// existe no projeto fica de fora do pedido — a conferência olha só a
    /// parte antes do `#`, então um caminho de verdade seguido de uma função
    /// inventada continua entrando.
    #[test]
    fn a_missing_must_read_path_is_left_out_of_the_request() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}],
                            "must_read": ["apps/rt/src/nao-existe.rs"]})),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        let read_before = crate::platform::i18n::translate("prompt.task.read_before", Locale::PtBr);
        assert!(!built[0].text.contains(read_before), "{}", built[0].text);
    }

    /// O pedido de uma onda, montado como a rodada o monta — com a cópia que
    /// ela criou para ela —, lista só os códigos: cada parte traz uma linha
    /// por bloco da spec, com os códigos em sequência, e as tarefas ganham
    /// linha própria, na ordem de execução que a onda declara. O comando de
    /// leitura aparece uma vez só, no exemplo, com o caminho do repositório
    /// principal, e nenhum item repete o comando nem o código.
    #[test]
    fn the_wave_request_lists_only_the_codes_per_block_with_one_example() {
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
        let wave = &built[0].text;

        // A onda tem `order: [8, 7]`: a tarefa 2 (id 8) vem antes da 1 (id 7).
        let tasks = ["`MSTD-TASK-0002`: `src/b.rs`", "`MSTD-TASK-0001`: `src/a.rs`"];
        // Onda, critérios, especificação e combinado saem juntos, sob um
        // título só: "Itens da onda".
        let items = section_lines(wave, part("prompt.part.items"));
        for code in [
            "`waves`: MSTD-WAVE-0001",
            "`criteria`: MSTD-CRIT-0001",
            "`specification`: MSTD-CTX-0001, MSTD-CTX-0002",
            "`agreed`: MSTD-DEC-0001, MSTD-RULE-0001",
        ] {
            assert!(items.contains(&code), "{code}: {wave}");
        }
        assert_eq!(section_lines(wave, part("prompt.part.tasks")), tasks, "{wave}");

        assert!(wave.contains(&example), "{wave}");
        for once in ["mustard-rt run read", "--term", "--root", "MSTD-TASK-0001", "MSTD-TASK-0002", "MSTD-WAVE-0001", "MSTD-CRIT-0001"] {
            assert_eq!(wave.matches(once).count(), 1, "{once}: {wave}");
        }
        for copied in ["O objetivo da obra.", "Montar a lista", "a lista sai curta", "A lista saiu."] {
            assert!(!wave.contains(copied), "{copied} foi copiado: {wave}");
        }
    }

    /// O pedido cai de quinze seções para quatro: os itens da onda (a
    /// própria onda, os critérios, a especificação e o combinado, tudo sob
    /// um título só), as tarefas, o que as ondas anteriores entregaram e as
    /// regras da execução — nenhum título a mais, e nenhum dos antigos
    /// (onda, critérios, especificação e combinado, cada um com o título
    /// próprio) sobra.
    #[test]
    fn the_wave_request_has_four_sections_not_fifteen() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let everywhere = json!({"files": ["**"]});
        let log = log_of(&[
            ("context", json!({"text": "O objetivo.", "keys": ["objetivo"], "label": "objetivo"})),
            ("decision", json!({"text": "Uma decisão.", "keys": ["decisão"], "why": "w", "applies_to": everywhere})),
            ("criterion", json!({"when": "a onda roda", "then": "a lista sai curta", "proof": "cargo test"})),
            ("wave", json!({"n": 1, "text": "A primeira onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Fazer o primeiro passo", "files": [{"path": "src/a.rs"}]})),
            ("delivered", json!({"wave": 1, "text": "Entregue.", "files": ["src/a.rs"]})),
            ("wave", json!({"n": 2, "text": "A onda", "criteria": [3], "done_when": "passa", "depends_on": [1]})),
            ("task", json!({"wave": 2, "text": "Fazer", "files": [{"path": "src/b.rs"}]})),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        let wave = &built.iter().find(|p| p.wave == 2).expect("onda 2 no plano").text;
        let part = |key: &str| crate::platform::i18n::translate(key, Locale::PtBr);
        let headings: Vec<String> = wave.lines().filter(|line| line.starts_with("## ")).map(String::from).collect();
        assert_eq!(
            headings,
            [
                format!("## {}", part("prompt.part.items")),
                format!("## {}", part("prompt.part.tasks")),
                format!("## {}", part("prompt.part.delivered")),
                format!("## {}", part("prompt.part.execution")),
            ],
            "{wave}"
        );
    }

    /// As linhas de uma seção do pedido, sem o "- " do começo; vazio quando
    /// a seção não está lá.
    fn section_lines<'a>(text: &'a str, heading: &str) -> Vec<&'a str> {
        let Some((_, after)) = text.split_once(&format!("## {heading}")) else { return Vec::new() };
        let section = after.split("\n## ").next().unwrap_or_default();
        section.lines().filter_map(|line| line.strip_prefix("- ")).collect()
    }

    /// Um banco com seis defeitos ligados às tarefas, dois sem palavra em
    /// comum com elas e uma preferência também sem: o pedido da onda leva só
    /// os cinco defeitos mais ligados, pelo número deles no banco — nenhum
    /// texto de lição entra no pedido. O sexto, que divide uma palavra só,
    /// perde o lugar; os sem palavra em comum ficam fora do pedido.
    #[test]
    fn the_wave_request_carries_only_the_lessons_tied_to_the_tasks() {
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
        // Ids 2 a 6 no banco: os cinco defeitos fortes, na ordem em que
        // entraram — `loose[0]` ocupa o 1, e o sexto e o oitavo (o fraco e o
        // outro solto) ficam fora.
        let expected: Vec<String> = (2u64..=6).map(|id| format!("`lessons`: {id}")).collect();
        let items = section_lines(&built[0].text, crate::platform::i18n::translate("prompt.part.items", Locale::PtBr));
        let lessons: Vec<&str> = items.iter().copied().filter(|line| line.starts_with("`lessons`:")).collect();
        assert_eq!(lessons, expected.iter().map(String::as_str).collect::<Vec<_>>(), "o pedido da onda: {}", built[0].text);
        for out in strong.iter().map(String::as_str).chain([weak, loose[0], loose[1], "Resposta curta."]) {
            assert!(!built[0].text.contains(out), "{out} não deve aparecer por texto: só o número entra");
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
    /// aparece na tarefa, continua entrando nos dois, pelo número dela no
    /// banco; a da trava, cuja palavra-chave "ao mesmo tempo" só aparece pela
    /// metade ("tempo"), e a da suíte ficam fora.
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
        let items = section_lines(&built[0].text, part("prompt.part.items"));
        let lessons: Vec<&str> = items.iter().copied().filter(|line| line.starts_with("`lessons`:")).collect();
        assert_eq!(lessons, ["`lessons`: 96"], "{lessons:?}");
        for out in [DISPATCH_LESSON, "Subagente nunca roda", "Quem tira uma proteção", "O teste tem de falhar"] {
            assert!(!built[0].text.contains(out), "{out} não deve aparecer por texto: só o número entra");
        }
    }

    /// A lição real sobre o envio do pedido de uma onda, do banco deste
    /// projeto.
    const DISPATCH_LESSON: &str = "O pedido da onda vai INTEIRO no corpo do prompt do subagente, como a rodada o devolve. Mandar no lugar dele um endereco — o codigo do envio gravado, ou um arquivo temporario — nao economiza contexto nenhum: a primeira coisa que o agente faz e abrir esse endereco e receber o mesmo texto de uma vez, so que em JSON cru e gastando uma chamada a mais. O que e preguicoso nao e o pedido, sao os itens dentro dele: cada linha da lista e um endereco, e o agente so abre o conteudo de um item quando chega a hora de trabalhar nele. Em 16/09 o usuario cortou dois agentes seguidos por isso, um por arquivo temporario e outro pelo codigo do envio.";

    /// A tarefa real da onda que só mexe na barra de status.
    const STATUS_BAR_TASK: &str = "A primeira linha da barra deixa de mostrar a contagem de linhas mudadas (+N-N) e fica igual ao exemplo aprovado: projeto, branch, spec, uso da conversa e tempo. O teste do exemplo aprovado passa a usar uma sessão com linhas mudadas e todos os dados que a sessão real traz, e reprova se a contagem voltar.";

    /// Uma pasta com 500 regras do projeto, e a onda mexe num arquivo dela: o
    /// pedido leva no máximo 5 regras, as mais ligadas ao texto das tarefas,
    /// pelo número delas no banco. A regra que só divide uma palavra com a
    /// tarefa perde o lugar para as que dividem quatro; das outras classes
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
        let items = section_lines(&built[0].text, crate::platform::i18n::translate("prompt.part.items", Locale::PtBr));
        let shown: Vec<&str> = items.iter().copied().filter(|line| line.starts_with("`lessons`:")).collect();
        // Ids 501 a 505: as cinco regras fortes, gravadas depois das 500
        // "Regra N"; id 509: a preferência, cujo texto também divide
        // "total" e "fatura" com a tarefa. Id 508, o defeito sem palavra em
        // comum, fica fora.
        let expected: Vec<String> =
            (501u64..=505).chain(std::iter::once(509)).map(|id| format!("`lessons`: {id}")).collect();
        assert_eq!(shown, expected.iter().map(String::as_str).collect::<Vec<_>>(), "{shown:?}");
        for out in strong.iter().chain(&weak).map(String::as_str).chain(["Apagar a pasta perde trabalho.", "O total da fatura sai em reais."]) {
            assert!(!built[0].text.contains(out), "{out} não deve aparecer por texto: só o número entra");
        }
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
            write_map(
                root,
                &json!({
                    "history": {
                        "paths": ["apps/rt/src/exemplo.rs"],
                        "commits": [{"id": "abc1234", "at": moved, "changed": [0]}],
                    }
                }),
            );
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
    /// está fora não fala de cópia. O projeto é Rust: o mapa marca a raiz
    /// como `cargo`.
    #[test]
    fn the_request_carries_the_copy_the_round_chose_or_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_map(root, &json!({"projects": [{"name": "(root)", "dir": "", "kind": "cargo"}]}));
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
        assert!(built[0].text.contains(&format!("--root {} --spec teste", shown(root))), "{}", built[0].text);

        let still = prompts(root, "teste", &log, Locale::PtBr, &Flight::default());
        assert!(!still[0].text.contains("/c/um") && !still[0].text.contains("CARGO_TARGET_DIR"), "{}", still[0].text);
        assert!(!still[0].text.contains("--root"), "sem cópia, o agente lê a spec de onde está: {}", still[0].text);
    }

    /// Os pedidos que a rodada monta — o da onda e o da revisão final — num
    /// projeto sem mapa, num só com parte Node, num com
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
                write_map(root, &json!({"projects": projects}));
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
                for (what, text) in [("wave", &built[0].text), ("final", &last)] {
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
