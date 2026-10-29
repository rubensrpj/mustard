//! O pedido de cada onda, montado do disco: o arquivo de eventos da spec, o
//! banco de lições, os arquivos das skills que as tarefas nomeiam, os
//! comandos de compilar, testar e preparar a cópia que o projeto declara e os
//! arquivos locais dele.
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
//! descrição do frontmatter. O caminho recomendado é o absoluto, no projeto
//! principal: a pasta `.claude/skills` costuma ficar fora do git e não chegar à cópia da
//! onda. A skill cujo arquivo citado mudou no git depois do arquivo dela sai
//! marcada como a revisar.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::domain::config::Setting;
use crate::domain::lessons::{Scope, in_scope, related_to_tasks, serving_wave};
use crate::domain::normalize::Languages;
use crate::domain::pattern::Pattern;
use crate::domain::project_map::{
    FileLineage, History, MAX_COMMITS, MOVES_FOLLOWED, MapModule, MapRefusal, ProjectMap, QualityCuts, Recipe, check_skill, cited_paths, examples_following,
    file_history, lineage_fresh_in, recipe_for_existing, recipe_for_new, recipe_from_lineage, tests_for,
};
use crate::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog};
use crate::domain::wave_prompt::{
    self, Choice, Execution, Material, PatternExample, Skill, SkillOption, SkillTie, TaskBlock, TaskPattern, TaskSkill, WaveCopy, tasks_text,
    wave_files,
};
use crate::io::project_map::{MapReader, Need};
use crate::platform::i18n::Locale;

mod changed;
mod prepared;
mod rules;

pub use rules::{project_rules, touched_files};

/// O pedido do revisor final, como o disco o entrega.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalReview {
    /// O texto do pedido.
    pub text: String,
    /// O que o pedido lista, para o veredito conferir a leitura: o código de
    /// cada item, uma vez só. É a lista que imprimiu as linhas do texto
    /// ([`wave_prompt::listed_final_review`]), não um segundo cálculo.
    pub listed: Vec<String>,
}

/// O pedido de uma onda, como o disco o entrega.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavePrompt {
    /// O número da onda.
    pub wave: u64,
    /// O nome do agente que recebe a onda: sempre `"wave"`, numa tarefa só
    /// ou em várias — o molde que o projeto instalou com esse nome.
    pub agent: String,
    /// O modelo pedido para a onda: o que o `mustard.json` declara para os
    /// agentes ([`Execution::requested_model`]).
    pub model: String,
    /// O esforço pedido para a onda: o que o `mustard.json` declara para os
    /// agentes ([`Execution::requested_effort`]).
    pub effort: String,
    /// De onde o modelo subiu, quando a onda volta reprovada
    /// ([`crate::domain::agent_model::wave_model`]).
    pub escalated_from: Option<crate::domain::agent_model::Escalation>,
    /// O texto do pedido, sempre: a página mostra mesmo o pedido recusado,
    /// que é justamente o que precisa ser visto antes da aprovação.
    pub text: String,
    /// Quantas linhas ele tem.
    pub lines: usize,
    /// O que o pedido lista, para a entrega conferir a leitura: o código de
    /// cada item e `lesson-<número>` de cada lição. É a lista que imprimiu as
    /// linhas do texto ([`wave_prompt::listed`]), não um segundo cálculo.
    pub listed: Vec<String>,
    /// O bloco de cada tarefa, na ordem de execução: o texto traz o da
    /// primeira, e o envio guarda todos para a resposta do passo entregar o
    /// seguinte ([`wave_prompt::task_blocks`]).
    pub tasks: Vec<TaskBlock>,
    /// As skills que a conferência recusou, pelo nome.
    pub bad_skills: Vec<(String, MapRefusal)>,
    /// As skills que a onda usa e que precisam de revisão, pelo nome.
    pub stale_skills: Vec<String>,
    /// Os números do `mustard.json` que a montagem leu, não valiam e caíram
    /// no padrão. A montagem não tem sessão: quem despacha o pedido decide
    /// onde o aviso sai e quantas vezes.
    pub bad_settings: Vec<BadSetting>,
    pub skills: Vec<Value>,
    /// As tarefas com mais skills candidatas do que cabem no bloco, ainda sem
    /// escolha ([`Flight::skill_picks`]): o bloco leva as primeiras.
    pub skill_ties: Vec<SkillTie>,
    /// As skills candidatas a alguma tarefa da onda, pelo nome, uma vez cada:
    /// a nomeada e as que casam com os arquivos dela.
    pub skill_candidates: Vec<String>,
}

impl WavePrompt {
    /// O maior texto que o agente da onda recebe de uma vez, em tokens
    /// ([`wave_prompt::estimate_tokens`]): o pedido ou o bloco de uma das
    /// tarefas seguintes, que chega na resposta de um passo. É o que o teto
    /// do pedido mede.
    #[must_use]
    pub fn tokens(&self) -> u64 {
        let blocks = self.tasks.iter().map(|block| wave_prompt::estimate_tokens(&block.text));
        blocks.chain(std::iter::once(wave_prompt::estimate_tokens(&self.text))).max().unwrap_or_default()
    }
}

/// Um número do `mustard.json` que o pedido leu e que não vale — zero,
/// negativo ou texto —, com o aviso pronto. Vale o padrão, e o aviso diz a
/// chave e o padrão que valeu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadSetting {
    /// A chave da seção `map`, como o arquivo a escreve (`historyMoves`).
    pub key: &'static str,
    /// O aviso, no idioma do texto.
    pub message: String,
}

/// As ondas que estão fora, como quem monta os pedidos as vê.
#[derive(Debug, Default)]
pub struct Flight {
    /// Actual caller host; a persisted send remains authoritative on replay.
    pub host: Option<String>,
    /// As ondas em andamento — a rodada conta também as que saem junto nela.
    /// O pedido de cada onda lista as outras, com os arquivos delas.
    pub running: BTreeSet<u64>,
    /// A cópia de cada onda que sai agora, que a rodada criou antes de gravar
    /// o envio. A cópia de uma onda que já saiu vem do envio gravado dela.
    pub copies: BTreeMap<u64, WaveCopy>,
    /// A escolha do orquestrador antes do envio de cada onda que sai agora.
    /// A de uma onda que já saiu vem do envio gravado dela.
    pub choices: BTreeMap<u64, Choice>,
    /// As skills escolhidas para o bloco de cada tarefa com mais candidatas
    /// do que cabem nele ([`WavePrompt::skill_ties`]), pelo número da tarefa:
    /// o caminho de cada uma, da de maior chance para a de menor. A lista
    /// vazia deixa só a nomeada pela tarefa.
    pub skill_picks: BTreeMap<u64, Vec<String>>,
}

/// Os pedidos das ondas `waves` que o plano tem, em ordem de número, com as
/// ondas que estão fora em `flight`. Cada pedido custa a leitura dos itens,
/// das lições e do mapa da onda: a rodada pede só as que saem agora, e a
/// conferência do plano, todas ([`SpecLog::planned_waves`]).
#[must_use]
pub fn prompts(root: &Path, spec: &str, log: &SpecLog, lang: Locale, flight: &Flight, waves: &BTreeSet<u64>) -> Vec<WavePrompt> {
    let trace = |file: &str, moves: usize| crate::Scan::locate().history(root, &crate::io::project_map::model_path(root), file, moves).is_ok();
    prompts_reading(root, spec, log, lang, flight, waves, &|need| crate::io::project_map::read_for(root, need), &trace)
}

/// Como a montagem lê do git, na hora, a história de um arquivo cujo último
/// commit ficou fora da janela do mapa, seguindo a declaração até `moves`
/// vezes para o arquivo de onde ela veio: pelo `scan history`, que a grava
/// no mapa. `true` quando gravou.
pub type Trace<'t> = dyn Fn(&str, usize) -> bool + 't;

/// A história do arquivo `file` além da janela do mapa, quando o último
/// commit dele ficou fora dela: a janela cheia, com a história da branch de
/// partida, e nenhum commit do arquivo nela. Vale a guardada no mapa, pela
/// mesma validade da pergunta da história de uma declaração; senão, `trace`
/// a lê do git na hora, seguindo até `moves` mudanças de arquivo, e a grava
/// no mapa, e a pergunta seguinte não relê o git. `None` quando a janela
/// ainda guarda a história inteira, quando o arquivo tem commit nela ou
/// quando a leitura falha.
fn history_beyond_window(read: &MapReader<'_>, trace: &Trace<'_>, history: &History, file: &str, moves: impl FnOnce() -> usize) -> Option<FileLineage> {
    if file_history(history, file).is_some() || history.commits.len() < MAX_COMMITS || history.missing.is_some() {
        return None;
    }
    let stored = |part: ProjectMap| part.lineage.into_iter().find(|lineage| lineage.path == file);
    let part = read(Need::Lineage(file)).ok()?;
    let comments = part.pulls.comments.len();
    let mark = part.census_mark.clone();
    // O número só se lê aqui, quando a história além da janela vai mesmo ser
    // comparada com a guardada ou lida do git.
    let moves = moves();
    if let Some(found) = stored(part).filter(|l| lineage_fresh_in(l, history, &mark, comments, moves)) {
        return Some(found);
    }
    if !trace(file, moves) {
        return None;
    }
    read(Need::Lineage(file)).ok().and_then(stored)
}

/// A receita do git do alvo `target` do projeto `root`, a mesma no pedido
/// da onda e nos exemplos do mapa. Quem diz se o alvo existe é o disco, não
/// o mapa, que pode estar atrás dele: o arquivo apagado que o mapa ainda
/// lista vai ser criado de novo, e o criado depois da última passada do scan
/// vai ser mudado.
///
/// - o caminho que termina em `/`, ou a pasta que existe: a receita de criar
///   um arquivo qualquer nela;
/// - o arquivo que não existe: a de criar um do tipo dele na pasta dele;
/// - o arquivo que existe: a de mudá-lo, pelos commits da janela do mapa
///   `map`; ou, quando o mapa o conhece e o último commit dele ficou fora
///   da janela, pela história lida do git na hora
///   ([`history_beyond_window`]), seguindo o número de mudanças de arquivo
///   que `moves` dá, pedido só quando essa leitura acontece.
///
/// O arquivo que mudou junto e não existe mais sai dela ([`recipe_on_disk`]).
pub fn recipe_for(root: &Path, read: &MapReader<'_>, trace: &Trace<'_>, map: &ProjectMap, target: &str, moves: impl FnOnce() -> usize) -> Option<Recipe> {
    let history = &map.history;
    let on_disk = root.join(target);
    let found = if target.ends_with('/') {
        recipe_for_new(history, target)
    } else if on_disk.is_dir() {
        recipe_for_new(history, &format!("{target}/"))
    } else if !on_disk.exists() {
        recipe_for_new(history, target)
    } else if file_history(history, target).is_some() {
        recipe_for_existing(history, target)
    } else if map.module(target).is_some() {
        history_beyond_window(read, trace, history, target, moves).as_ref().and_then(recipe_from_lineage)
    } else {
        None
    };
    found.and_then(|recipe| recipe_on_disk(root, recipe))
}

/// A receita `recipe` do jeito que ela chega a quem a lê, no projeto `root`:
/// o arquivo que mudou junto e não existe mais sai dela, e a receita que fica
/// sem nada a dizer, sem arquivo junto e sem teste, não sai.
fn recipe_on_disk(root: &Path, recipe: Recipe) -> Option<Recipe> {
    recipe_keeping(recipe, |path| root.join(path).exists())
}

/// A receita `recipe` só com os arquivos que mudaram junto que `keep`
/// guarda; nenhuma, quando ela fica sem arquivo junto e sem teste.
fn recipe_keeping(mut recipe: Recipe, keep: impl Fn(&str) -> bool) -> Option<Recipe> {
    recipe.together.retain(|(path, _)| keep(path));
    (!recipe.together.is_empty() || recipe.tests.is_some()).then_some(recipe)
}

/// Os pedidos de [`prompts`], com o mapa do projeto lido por `read`, cada
/// parte pela pergunta dela, e a história além da janela lida por `trace`.
#[allow(clippy::too_many_arguments)]
fn prompts_reading(
    root: &Path,
    spec: &str,
    log: &SpecLog,
    lang: Locale,
    flight: &Flight,
    waves: &BTreeSet<u64>,
    read: &MapReader<'_>,
    trace: &Trace<'_>,
) -> Vec<WavePrompt> {
    let bank = lesson_bank(root);
    let base = project_execution(root);
    let languages = Languages::of_project(root);
    let map = MapParts::new(root, read, trace);
    let context = Context { root, spec, log, bank: bank.as_ref(), map: &map, base: &base, flight, lang, languages: &languages };
    let mut built: Vec<WavePrompt> = log.planned_waves().intersection(waves).map(|n| one(&context, *n)).collect();
    // O aviso do número inválido vai em todo pedido da montagem, e não só no
    // da onda cuja receita o leu primeiro: quem despacha pega os pedidos das
    // ondas que saem agora, e a receita guardada serve às seguintes.
    if map.moves_bad_read.get() {
        let bad = BadSetting {
            key: MOVES_KEY,
            message: crate::platform::i18n::translate("map.history.bad_setting", lang)
                .replace("{key}", MOVES_KEY)
                .replace("{default}", &MOVES_FOLLOWED.to_string()),
        };
        for prompt in &mut built {
            prompt.bad_settings.push(bad.clone());
        }
    }
    built
}

/// A chave de `map.historyMoves` no `mustard.json`, como o aviso do valor
/// inválido a diz.
const MOVES_KEY: &str = "historyMoves";

/// O que os dois pedidos leem do `mustard.json` do projeto `root`: os
/// comandos de compilar e de testar, o de preparo e os idiomas, com o
/// repositório principal.
fn project_execution(root: &Path) -> Execution {
    let config = crate::ProjectConfig::load(root);
    let commands = config.commands();
    Execution {
        build: commands.build,
        test: commands.test,
        prepare: commands.prepare,
        root: shown(root),
        language: config.language(),
        model: config.wave_agent_model().to_string(),
        // O esforço é sempre o padrão, em todo agente e modelo: o lançamento
        // o impõe, e o pedido diz o mesmo.
        effort: crate::domain::config::DEFAULT_AGENT_EFFORT.to_string(),
        by_difficulty: config.by_difficulty(),
        ..Execution::default()
    }
}

/// Os arquivos locais que o projeto declara (`localFiles`) e que uma cópia
/// pode receber ([`local_file_inside`]), na ordem da lista, sem as entradas
/// em branco. Vazia quando a lista falta ou está vazia.
#[must_use]
pub fn local_files(config: &crate::ProjectConfig) -> Vec<String> {
    config.local_files.iter().flatten().map(|file| file.trim()).filter(|file| local_file_inside(file)).map(str::to_string).collect()
}

/// O item da lista de arquivos locais é um caminho relativo que fica dentro
/// do projeto: nem absoluto, nem com `..`, nem vazio. Só esse chega a uma
/// cópia; o outro escreveria fora dela, ou sobre o próprio arquivo do
/// repositório principal.
#[must_use]
pub fn local_file_inside(item: &str) -> bool {
    use std::path::Component;
    let path = Path::new(item);
    path.components().all(|part| matches!(part, Component::Normal(_) | Component::CurDir)) && path.components().any(|part| matches!(part, Component::Normal(_)))
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
/// delas passaria do teto de tokens do pedido, e a lição sem palavra em comum
/// com a tarefa só ocupa o agente. Delas, ficam só as que servem à onda: a
/// que cita um arquivo vai só à onda que mexe nele, e a onda só de texto não
/// recebe lição do projeto todo nem do subprojeto ([`serving_wave`]). São as
/// que a rodada mostra ao orquestrador antes do envio, e as que o pedido
/// leva, menos as que a escolha dele tirou. A lição que diz os arquivos onde
/// vale chega assim a toda onda que mexe neles, e por isso o levantamento
/// não pergunta por ela ([`crate::domain::lessons::reaches_waves_by_files`]).
#[must_use]
pub fn wave_lessons<'a>(bank: &'a SpecLog, log: &SpecLog, wave: u64, languages: &Languages) -> Vec<&'a SpecEvent> {
    let files = wave_files(log, wave);
    let skills = skills_named(log, wave);
    let mut found: Vec<&SpecEvent> = Vec::new();
    for skill in std::iter::once(None).chain(skills.iter().cloned().map(Some)) {
        let scope = Scope { files: files.clone(), subproject: None, skill };
        for lesson in in_scope(bank, &scope) {
            if !found.iter().any(|seen| seen.id == lesson.id) {
                found.push(lesson);
            }
        }
    }
    serving_wave(related_to_tasks(found, &tasks_text(log, wave), languages), &files, &skills)
}

/// A pasta das cópias separadas do projeto do checkout principal `root`,
/// fora da pasta dele: é o único lugar onde nascem a cópia de cada onda e a
/// do revisor final. Dentro do projeto, as ferramentas dele — o lint do
/// gancho de commit, o editor — enxergariam a cópia como parte do projeto.
///
/// A base é a pasta que `MUSTARD_COPIES_DIR` indicar, ou a pasta de cache do
/// usuário, `.cache/mustard/copias` sob a pasta pessoal
/// ([`crate::platform::harness::home_dir`]); sem nenhuma das duas, a pasta
/// temporária do sistema. Dentro dela, uma
/// pasta por projeto: o nome da pasta do checkout e um código curto do
/// caminho dele, que separa dois projetos de mesmo nome e não muda de uma
/// chamada para outra — o caminho é lido já resolvido, então o atalho e a
/// barra invertida não mudam o código.
#[must_use]
pub fn copies_dir(root: &Path) -> PathBuf {
    project_folder(root, "MUSTARD_COPIES_DIR", "copias")
}

/// A pasta `path` mora dentro de um dos lugares de cópia do projeto `root`:
/// a pasta das cópias dele ([`copies_dir`]), fora do projeto, ou o lugar
/// antigo, dentro dele (`.claude/worktrees`). Só aí uma pasta pode ser
/// apagada; a pasta principal e a compilação dela nunca entram, nem o caminho
/// que traz `..`.
///
/// As duas pontas se comparam pela mesma grafia, a do lugar de verdade
/// ([`real_place`]): a pasta do terminal, que o sistema dá já resolvida, e a
/// pasta das cópias, montada da pasta pessoal ou da variável, se encontram
/// mesmo com um link no caminho de uma delas ou com o prefixo de caminho
/// literal do Windows.
#[must_use]
pub fn inside_copies(root: &Path, path: &Path) -> bool {
    if path.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
        return false;
    }
    let here = real_place(path);
    [copies_dir(root), root.join(".claude").join("worktrees")]
        .iter()
        .map(|place| real_place(place))
        .any(|place| here.starts_with(&place) && here != place)
}

/// O lugar de verdade que `path` nomeia: a parte que existe resolvida pelo
/// sistema — o link, a pasta pessoal atrás de um link, o nome curto do
/// Windows —, a que ainda não existe como foi escrita, e sem o prefixo de
/// caminho literal que o Windows põe no que resolve ([`without_verbatim`]).
#[must_use]
pub fn real_place(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut tail = Vec::new();
    let mut head = absolute.as_path();
    loop {
        if let Ok(real) = std::fs::canonicalize(head) {
            let plain = without_verbatim(&real.to_string_lossy());
            let real = plain.map_or(real, PathBuf::from);
            return tail.iter().rev().fold(real, |full, part| full.join(part));
        }
        match (head.file_name(), head.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_os_string());
                head = parent;
            }
            _ => return absolute,
        }
    }
}

/// O texto do caminho `text` sem o prefixo de caminho literal do Windows, que
/// o caminho resolvido pelo sistema traz: `\\?\C:\x` é `C:\x` e
/// `\\?\UNC\srv\x` é `\\srv\x`; o prefixo de aparelho (`\\.\`) sai do mesmo
/// jeito. É a regra única do prefixo, de quem resolve o lugar de verdade
/// ([`real_place`]) e de quem compara caminhos pelo texto.
///
/// As barras podem ser as do Windows ou as normais (`//?/C:/x` é `C:/x`), e o
/// compartilhamento recebe as barras do próprio texto. O prefixo seguido de
/// outra coisa que um disco ou um compartilhamento — o volume,
/// `\\?\Volume{…}\x` — fica: sem ele o caminho não diz lugar nenhum. `None`
/// quando não há prefixo a tirar.
#[must_use]
pub fn without_verbatim(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let slash = |at: usize| matches!(bytes.get(at), Some(b'\\' | b'/'));
    if !(slash(0) && slash(1) && matches!(bytes.get(2), Some(b'?' | b'.')) && slash(3)) {
        return None;
    }
    let rest = &text[4..];
    if rest.get(..3) == Some("UNC") && slash(7) {
        let separator = &text[..1];
        return Some(format!("{separator}{separator}{}", &text[8..]));
    }
    let drive = rest.as_bytes();
    (drive.len() >= 2 && drive[0].is_ascii_alphabetic() && drive[1] == b':').then(|| rest.to_string())
}

/// A pasta em que o terminal está, como o sistema a dá: a leitura única de
/// quem pergunta se a chamada veio de dentro de uma cópia — o aviso da rodada
/// e a chamada de dentro da cópia da spec. `None` quando não se sabe. Quem a
/// compara com as cópias é [`inside_copies`], pela mesma grafia.
#[must_use]
pub fn terminal_folder() -> Option<PathBuf> {
    std::env::current_dir().ok()
}

/// A pasta onde nasce o programa compilado da versão em construção do
/// projeto do checkout principal `root`: o `mustard-rt`, o `scan` e o
/// `mustard` ficam em `<pasta>/release/`.
///
/// Mora ao lado das cópias, com a mesma chave por projeto de [`copies_dir`], e
/// fora da pasta de compilação que o fechamento apaga: o programa que a sessão
/// roda tem de sobreviver ao fechamento da spec. A base é a pasta que
/// `MUSTARD_BUILD_DIR` indicar, ou `.cache/mustard/build` sob a pasta pessoal;
/// sem nenhuma das duas, a pasta temporária do sistema.
#[must_use]
pub fn development_build_dir(root: &Path) -> PathBuf {
    project_folder(root, "MUSTARD_BUILD_DIR", "build")
}

/// O arquivo onde mora o retrato do mapa do código como a base estava, da
/// spec `spec` do projeto `root`: a análise de declarações sem uso compara o
/// mapa de agora com ele.
///
/// Mora fora do projeto, ao lado das cópias, com a mesma chave por projeto de
/// [`copies_dir`]. A base é a pasta que `MUSTARD_MAPS_DIR` indicar, ou
/// `.cache/mustard/mapas` sob a pasta pessoal. Ele ficava dentro da pasta da
/// spec, e lá chegou a 89 MB numa máquina de verdade, sem o fechamento
/// apagá-lo: um arquivo desse tamanho não é estado da obra, é cache, e cache
/// não mora no projeto.
#[must_use]
pub fn validation_base_map(root: &Path, spec: &str) -> PathBuf {
    project_folder(root, "MUSTARD_MAPS_DIR", "mapas").join(format!("{spec}.json"))
}

/// O nome do arquivo do mapa da base dentro da pasta de uma spec, de antes de
/// ele sair do projeto: a limpeza o recolhe das specs que já acabaram.
pub const OLD_VALIDATION_MAP: &str = "validation-base-map.json";

/// A pasta da cópia de segurança das skills que o Mustard criou no projeto do
/// checkout principal `root`, uma pasta por skill: mora fora do projeto, com
/// a mesma chave por projeto de [`copies_dir`]. A base é a pasta que
/// `MUSTARD_SKILLS_DIR` indicar, ou `.cache/mustard/skills` sob a pasta
/// pessoal.
#[must_use]
pub fn skills_backup_dir(root: &Path) -> PathBuf {
    project_folder(root, "MUSTARD_SKILLS_DIR", "skills")
}

/// A pasta, na raiz da cópia de uma onda, em que a onda da tarefa de criar ou
/// de atualizar uma skill escreve a versão nova dela: a pasta das skills do
/// projeto fica fora da cópia, e a rodada grava a versão nova no lugar da
/// skill quando assume a volta. Nada dela entra num commit.
pub const RESERVED_SKILLS: &str = ".mustard-skill";

/// O caminho, relativo à raiz da cópia, em que a onda escreve a versão nova da
/// skill `name` ([`RESERVED_SKILLS`]).
#[must_use]
pub fn reserved_skill(name: &str) -> String {
    format!("{RESERVED_SKILLS}/{name}/SKILL.md")
}

/// A pasta de `leaf` do projeto de `root`: a base que a variável `var`
/// indicar, ou `.cache/mustard/<leaf>` sob a pasta pessoal, ou a pasta
/// temporária, e dentro dela `<nome>-<código>`. A chave é uma só para toda
/// pasta por projeto, para que duas delas nunca discordem sobre qual projeto
/// é qual.
fn project_folder(root: &Path, var: &str, leaf: &str) -> PathBuf {
    let base = std::env::var_os(var)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::platform::harness::home_dir().map(|home| home.join(".cache").join("mustard").join(leaf)))
        .unwrap_or_else(|| std::env::temp_dir().join("mustard").join(leaf));
    let main = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let name = main.file_name().map_or_else(|| String::from("projeto"), |name| name.to_string_lossy().into_owned());
    let code = crate::platform::fingerprint::fingerprint(&shown(&main)) >> 32;
    base.join(format!("{name}-{code:08x}"))
}

/// A pasta das cópias da spec `spec`, dentro da pasta das cópias do projeto
/// ([`copies_dir`]): cada vaga da obra mora nela, e o fechamento e o
/// descarte a apagam inteira.
#[must_use]
pub fn spec_copies_dir(root: &Path, spec: &str) -> PathBuf {
    copies_dir(root).join(spec)
}

/// O nome da vaga de número `slot`, contado do zero: `a` a `z` e, daí em
/// diante, o número dela contado do um (`27`, `28`…).
#[must_use]
pub fn slot_name(slot: usize) -> String {
    match u8::try_from(slot).ok().filter(|n| *n < 26) {
        Some(n) => char::from(b'a' + n).to_string(),
        None => (slot + 1).to_string(),
    }
}

/// A pasta da vaga de número `slot` da spec `spec`: a cópia fixa que passa de
/// uma onda para a seguinte, com a compilação dentro dela. Quem a cria e a
/// zera é a rodada.
#[must_use]
pub fn slot_path(root: &Path, spec: &str, slot: usize) -> PathBuf {
    spec_copies_dir(root, spec).join(slot_name(slot))
}

/// A cópia `copy`, como o envio a grava, é uma vaga da spec `spec`: mora
/// direto na pasta das cópias dela. O endereço antigo, `<spec>-<onda>` ao
/// lado das outras specs, não é vaga.
#[must_use]
pub fn is_slot_of(root: &Path, spec: &str, copy: &str) -> bool {
    let parent = Path::new(copy).parent().map(shown);
    parent.as_deref() == Some(shown(&spec_copies_dir(root, spec)).as_str())
}

/// A cópia limpa da spec `spec`, fora das vagas das ondas: a máquina do
/// fechamento roda nela, e o revisor final confere a obra nela, no mesmo
/// commit. Quem a prepara é o fechamento (`mustard-rt run close`); o pedido
/// do revisor só diz onde ela está.
#[must_use]
pub fn check_copy_path(root: &Path, spec: &str) -> PathBuf {
    spec_copies_dir(root, spec).join("check")
}

/// O commit em que o revisor final confere a obra: o mais novo que a spec
/// gravou no repositório principal do checkout `root`. O commit de um
/// submódulo, que a rodada grava com o nome da pasta dele em `repo`, não
/// existe no principal, e a cópia não nasceria nele. `None` quando a obra
/// ainda não comitou nada ali — aí o pedido diz `HEAD`, e o fechamento cria a
/// cópia no commit atual do checkout.
#[must_use]
pub fn final_review_commit(root: &Path, log: &SpecLog) -> Option<String> {
    let main = root.file_name().map(|name| name.to_string_lossy().to_string());
    log.block(BlockQuery::Block(Block::Progress))
        .into_iter()
        .rev()
        .filter(|e| e.event_type == "commit")
        .filter(|e| e.str_field("repo").is_none_or(|repo| Some(repo) == main.as_deref()))
        .find_map(|e| e.str_field("sha").map(str::to_string))
}


/// O pedido do agente de teste dedicado da spec `spec`, que o fechamento pede
/// a toda obra, mesmo a de uma onda só, no lugar da revisão de cada onda: só
/// o que nenhuma prova verde cobre ([`wave_prompt::Proven`]) — as tarefas sem
/// passo de término, o combinado sem onda que o cubra e os critérios sem
/// execução que passou — e a cópia limpa em que a máquina do fechamento
/// rodou ([`check_copy_path`]), no commit mais novo da spec. No fim vêm as regras do projeto: os
/// arquivos de regras da raiz e das pastas dos arquivos que a obra mexe
/// ([`project_rules`], [`touched_files`]), quando algum existe com texto.
///
/// Depois de um veredito final, o pedido é o da revisão de volta: ele lista
/// o que mudou desde esse veredito ([`wave_prompt::since_last_verdict`]), e o
/// revisor confere só a cadeia de cada conserto — a regra, os lugares e a
/// prova, que a linha de cada tarefa traz ([`wave_prompt::task_chains`]) —,
/// sem caçar achado no resto da obra nem repetir o que o veredito já
/// concluiu. Depois de uma aprovação — a obra fechada e reaberta para um
/// ajuste —, o que ela conferiu e não mudou sai também das outras partes
/// ([`wave_prompt::Proven`]), e o pedido traz só o ajuste. Depois de uma
/// reprovação, é o mesmo pedido quando o veredito aponta uma onda: ele
/// aparece uma vez só, na parte do que mudou, sem parte de conserto à parte.
/// Nesse caso, as ondas e as entregas do pedido ficam restritas à onda
/// reprovada, que já entregou o conserto (o fechamento só chega aqui depois
/// disso: veja [`crate::domain::spec_events::SpecLog::last_rejected`]), e o
/// parágrafo do que olhar da revisão de volta diz esse recorte. O recorte sai
/// do veredito reprovado mais novo ([`wave_prompt::last_final_rejection`]):
/// ele nunca acontece sem a revisão de volta. Uma onda reprovada antes de um
/// veredito final mais novo não recorta o pedido. Os requisitos acordados
/// continuam no pedido, menos os que a última aprovação já conferiu, porque o
/// veredito responde por cada um.
///
/// Junto do texto vem a lista dos itens que ele lista ([`FinalReview::listed`]),
/// para o fechamento gravá-la no envio e o veredito conferir a leitura. Os
/// itens saem só da spec ([`final_review_items`]); o disco dá a cópia, o
/// commit, as regras da execução e as do projeto, que o texto leva e a lista
/// não.
///
/// Só lê o disco, nunca grava: o fechamento grava o envio com o que ela
/// devolve, e a leitura `request-review-preview` imprime o mesmo texto sem
/// gravar nada, para o usuário ver o pedido real antes de ele valer.
#[must_use]
pub fn final_review(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> FinalReview {
    let commit = final_review_commit(root, log);
    let copy = check_copy_path(root, spec);
    let prepared = crate::io::workspace::checkout_git_dir(&copy).is_some_and(|admin| admin.join("mustard-dependencies.sha256").is_file());
    let execution = Execution { commit, prepared, copy: Some(WaveCopy { path: shown(&copy), reused: None }), ..project_execution(root) };
    let mut material = Material {
        spec: spec.to_string(),
        execution,
        project_rules: project_rules(root, &touched_files(root, Some(log))),
        ..final_review_items(log)
    };
    let tasks = material.since_verdict.iter().chain(&material.block).copied();
    material.chains = wave_prompt::task_chains(log, tasks, material.execution.test.as_deref());
    FinalReview { text: wave_prompt::write_final_review(&material, lang), listed: wave_prompt::listed_final_review(&material) }
}

/// O que o pedido da revisão final lista ([`FinalReview::listed`]), tirado só
/// da spec, sem montar o pedido: nem texto, nem cópia, nem git, nem regras do
/// projeto. O painel, a rodada e o fechamento perguntam daqui se o pedido
/// gravado ficou para trás de um item novo; a lista é a mesma que
/// [`final_review`] grava no envio, porque as duas saem dos mesmos itens.
#[must_use]
pub fn final_review_listed(log: &SpecLog) -> Vec<String> {
    wave_prompt::listed_final_review(&final_review_items(log))
}

/// Os itens do pedido da revisão final, só da spec. Só o que nenhuma prova
/// verde cobre: a tarefa sem passo de término — da onda reprovada, no
/// conserto, ou de todo o plano —, o combinado que nenhuma onda cobre e o
/// critério sem execução que passou. A onda, a entrega, o commit e o recibo
/// da máquina já são prova, e o revisor os acha na cópia. Depois de um
/// veredito final, também o que mudou desde ele
/// ([`wave_prompt::since_last_verdict`]).
fn final_review_items(log: &SpecLog) -> Material<'_> {
    let planned = log.planned_waves();
    let fixing: BTreeSet<u64> = wave_prompt::last_final_rejection(log).and_then(SpecEvent::wave).filter(|n| planned.contains(n)).into_iter().collect();
    let scope: &BTreeSet<u64> = if fixing.is_empty() { &planned } else { &fixing };
    let proven = wave_prompt::Proven::of(log);
    let visible = log.block(BlockQuery::Block(Block::Waves));
    let block: Vec<&SpecEvent> = visible
        .iter()
        .copied()
        .filter(|e| e.event_type == "task" && e.wave().is_some_and(|n| scope.contains(&n)))
        .filter(|e| !proven.covers(e))
        .collect();
    let criteria: Vec<&SpecEvent> =
        log.block(BlockQuery::Block(Block::Criteria)).into_iter().filter(|e| e.event_type == "criterion" && !proven.covers(e)).collect();
    let mut agreed: Vec<&SpecEvent> = wave_prompt::all_agreed(log).into_iter().filter(|e| !proven.covers(e)).collect();
    agreed.sort_by_key(|e| e.id);
    Material { block, criteria, agreed, since_verdict: wave_prompt::since_last_verdict(log), codes: log.codes(), ..Material::default() }
}

/// A cópia gravada no envio mais novo da onda `wave`. `None` quando esse
/// envio não criou cópia. O envio antigo que gravou também uma pasta de
/// compilação à parte é lido do mesmo jeito: só a cópia conta.
#[must_use]
pub fn recorded_copy(log: &SpecLog, wave: u64) -> Option<WaveCopy> {
    let sent = log.last_by_wave("send").get(&wave).and_then(|id| log.get(*id))?;
    let path = sent.str_field("copy")?.to_string();
    Some(WaveCopy { path, reused: None })
}

/// Um caminho como o pedido e o envio gravado o mostram: sempre com barras
/// normais, que o terminal e o controle de versão aceitam nos três sistemas.
#[must_use]
pub fn shown(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// O que o pedido de uma onda lista: os itens da spec e as lições do banco.
#[derive(Debug, Default)]
pub struct RequestItems<'a> {
    /// Os itens da spec, em ordem de número.
    pub items: Vec<&'a SpecEvent>,
    /// As lições do banco, em ordem de número.
    pub lessons: Vec<&'a SpecEvent>,
    /// O que as tarefas da onda atendem, pelo número com que a tarefa o cita
    /// ([`wave_prompt::attended`]). Também está em `items`.
    pub attended: BTreeMap<u64, &'a SpecEvent>,
}

/// O que o pedido da onda `wave` lista, pela mesma conta que o monta: os
/// itens que a montagem escolhe ([`wave_prompt::dispatch_items`]), o que as
/// tarefas da onda atendem — o critério, o combinado e a mensagem que elas
/// citam — e as lições que casam com a onda ([`wave_lessons`]), menos as que
/// a escolha tirou. A escolha é a dada (`fresh`) ou, sem ela, a gravada no
/// envio da onda — a mesma do pedido que a rodada mandou.
///
/// O envio, a entrega e os passos da própria onda ficam de fora: são o
/// registro de um pedido, não parte dele. A entrega que a reprovação julgou
/// fica, porque o conserto a cita ([`wave_prompt::fix_lines`]); a entrega das
/// ondas de que esta depende também, e o resumo que a onda continua
/// ([`wave_prompt::summary_of`]). O item combinado que uma versão nova
/// substituiu não entra, mesmo que a escolha o tenha posto: a escolha só vale
/// dentro dos candidatos de agora ([`wave_prompt::Choice::within`]), e a versão
/// antiga já não é candidata.
#[must_use]
pub fn request_items<'a>(log: &'a SpecLog, bank: Option<&'a SpecLog>, wave: u64, fresh: Option<&Choice>, languages: &Languages) -> RequestItems<'a> {
    let choice = wave_prompt::choice_for(log, wave, fresh);
    let fix: BTreeSet<u64> = wave_prompt::fix_lines(log, wave).iter().map(|e| e.id).collect();
    let attended = wave_prompt::attended(log, wave);
    let mut items: Vec<&SpecEvent> = wave_prompt::dispatch_items(log, wave, choice.as_ref())
        .into_iter()
        .filter(|e| match e.event_type.as_str() {
            "send" | "step" => false,
            "delivered" => e.wave() != Some(wave) || fix.contains(&e.id),
            _ => true,
        })
        .collect();
    // O resumo que a onda continua é o primeiro item que o pedido manda ler.
    for item in attended.values().copied().chain(wave_prompt::summary_of(log, wave)) {
        if !items.iter().any(|had| had.id == item.id) {
            items.push(item);
        }
    }
    items.sort_by_key(|e| e.id);
    let lessons = bank
        .map(|bank| wave_lessons(bank, log, wave, languages))
        .unwrap_or_default()
        .into_iter()
        .filter(|lesson| !choice.as_ref().is_some_and(|choice| choice.removes_lesson(lesson.id)))
        .collect();
    RequestItems { items, lessons, attended }
}

/// O mapa do projeto como o pedido o lê: cada parte pela pergunta dela, sem
/// o mapa inteiro. Os caminhos e a história se leem uma vez só por montagem;
/// as declarações e os testes, a cada arquivo que pede. Um mapa que falta ou
/// não se entende responde nada, como se não houvesse mapa.
struct MapParts<'a> {
    root: &'a Path,
    /// Quantas vezes a história de uma declaração segue para o arquivo de
    /// onde ela veio (`map.historyMoves`), como o `mustard.json` a escreve.
    /// Lida só por [`Self::moves`].
    moves: Setting,
    /// Se a montagem pediu o número e ele estava inválido: o pedido leva o
    /// aviso ([`BadSetting`]) só quando o padrão valeu de fato.
    moves_bad_read: Cell<bool>,
    trace: &'a Trace<'a>,
    /// A receita do git de cada arquivo de tarefa, pelo caminho, calculada
    /// uma vez só por montagem.
    recipes: RefCell<BTreeMap<String, Option<Recipe>>>,
    read: &'a MapReader<'a>,
    paths: OnceCell<Option<ProjectMap>>,
    summary: OnceCell<Option<ProjectMap>>,
    pattern: OnceCell<Option<ProjectMap>>,
    learned: OnceCell<Option<Pattern>>,
    /// Os exemplos de cada arquivo de tarefa, pelo caminho, e o exemplo de
    /// cada arquivo escolhido: o mesmo arquivo volta em várias ondas, e se
    /// calcula uma vez só por montagem.
    picks: RefCell<BTreeMap<String, Vec<String>>>,
    examples: RefCell<BTreeMap<String, Option<PatternExample>>>,
}

impl<'a> MapParts<'a> {
    /// O mapa do projeto `root` lido por `read`, sem nada lido ainda, com a
    /// história além da janela lida por `trace`. É o mesmo para o pedido da
    /// onda e para a medida do padrão, que precisam ler as mesmas partes do
    /// mesmo jeito.
    fn new(root: &'a Path, read: &'a MapReader<'a>, trace: &'a Trace<'a>) -> Self {
        Self {
            root,
            moves: crate::ProjectConfig::load(root).history_moves(),
            moves_bad_read: Cell::new(false),
            trace,
            recipes: RefCell::default(),
            read,
            paths: OnceCell::new(),
            summary: OnceCell::new(),
            pattern: OnceCell::new(),
            learned: OnceCell::new(),
            picks: RefCell::default(),
            examples: RefCell::default(),
        }
    }
}

impl MapParts<'_> {
    /// Quantas vezes a história segue para o arquivo de origem: o número do
    /// `mustard.json` ou, sem ele ou inválido, o padrão. O inválido deixa
    /// marcado que o padrão valeu no lugar dele.
    fn moves(&self) -> usize {
        if self.moves == Setting::Invalid {
            self.moves_bad_read.set(true);
        }
        self.moves.or(MOVES_FOLLOWED)
    }

    /// O caminho de cada arquivo do mapa, na ordem dele.
    fn paths(&self) -> Option<&ProjectMap> {
        self.paths.get_or_init(|| (self.read)(Need::Paths).ok()).as_ref()
    }

    /// A história do git que o mapa guarda.
    fn history(&self) -> Option<&History> {
        self.summary.get_or_init(|| (self.read)(Need::Summary).ok()).as_ref().map(|map| &map.history)
    }

    /// As declarações chamadas `name` no arquivo `file`, com ele.
    fn declarations(&self, file: &str, name: &str) -> Option<ProjectMap> {
        (self.read)(Need::Declarations { file: Some(file), name }).ok()
    }

    /// O arquivo `file` com os testes que o cobrem.
    fn tests(&self, file: &str) -> Option<ProjectMap> {
        (self.read)(Need::Tests(file)).ok()
    }

    /// O que o padrão do projeto e os exemplos leem: cada arquivo com as
    /// importações, o tamanho, a classe, os testes e os nomes que declara, a
    /// história e os subprojetos. Lido uma vez só por montagem.
    fn pattern(&self) -> Option<&ProjectMap> {
        self.pattern
            .get_or_init(|| {
                let mut map = (self.read)(Need::Examples { words: true }).ok()?;
                let summary = self.summary.get_or_init(|| (self.read)(Need::Summary).ok()).as_ref();
                map.projects = summary.map(|s| s.projects.clone()).unwrap_or_default();
                Some(map)
            })
            .as_ref()
    }

    /// O exemplo do padrão no arquivo `path` ([`pattern_example`]),
    /// calculado uma vez só por montagem.
    fn example(&self, read: &ProjectMap, path: &str) -> Option<PatternExample> {
        let known = self.examples.borrow().get(path).cloned();
        known.unwrap_or_else(|| {
            let found = pattern_example(self, read, path);
            self.examples.borrow_mut().insert(path.to_string(), found.clone());
            found
        })
    }

    /// A receita do git do arquivo `file` de uma tarefa, pela mesma escolha
    /// dos exemplos do mapa ([`recipe_for`]). Calculada uma vez só por
    /// montagem.
    fn recipe(&self, read: &ProjectMap, file: &str) -> Option<Recipe> {
        if let Some(known) = self.recipes.borrow().get(file) {
            return known.clone();
        }
        let found = recipe_for(self.root, self.read, self.trace, read, file, || self.moves());
        self.recipes.borrow_mut().insert(file.to_string(), found.clone());
        found
    }

    /// O padrão do projeto aprendido de [`Self::pattern`], com os arquivos
    /// `new_files` que o mapa ainda não tem: o papel de um arquivo que uma
    /// tarefa cria sai do nome dele, como o de um que já existe. Aprendido
    /// uma vez só por montagem.
    fn learned(&self, new_files: impl FnOnce() -> Vec<String>) -> Option<&Pattern> {
        self.learned
            .get_or_init(|| {
                let read = self.pattern()?;
                let mut with_new = read.clone();
                for path in new_files() {
                    if with_new.module(&path).is_none() {
                        with_new.modules.push(MapModule { path, ..MapModule::default() });
                    }
                }
                Some(crate::domain::pattern::learn(&with_new))
            })
            .as_ref()
    }
}

/// O que é igual para o pedido de todas as ondas de uma montagem.
struct Context<'a> {
    root: &'a Path,
    spec: &'a str,
    log: &'a SpecLog,
    bank: Option<&'a SpecLog>,
    map: &'a MapParts<'a>,
    /// Os comandos do projeto.
    base: &'a Execution,
    flight: &'a Flight,
    lang: Locale,
    /// As línguas em que as palavras das buscas são cortadas.
    languages: &'a Languages,
}

fn one(context: &Context, wave: u64) -> WavePrompt {
    let Context { root, spec, log, bank, map, lang, languages, .. } = *context;
    // O que a montagem escolhe, com a escolha do orquestrador antes do envio:
    // a que a rodada traz agora ou a gravada no envio da onda.
    let fresh = context.flight.choices.get(&wave);
    // Os itens e as lições saem da mesma conta que a leitura do pedido usa,
    // e por isso as duas nunca discordam.
    let RequestItems { items: read, lessons, attended } = request_items(log, bank, wave, fresh, languages);
    let of_type = |name: &str| -> Vec<&SpecEvent> { read.iter().copied().filter(|e| e.event_type == name).collect() };
    // A onda e as tarefas dela, de onde saem a frase que abre o pedido, a
    // ordem de execução e um passo por tarefa; o resto do que ela cita
    // entra pelo que as tarefas atendem.
    let block: Vec<&SpecEvent> = read.iter().copied().filter(|e| e.block() == Some(Block::Waves) && matches!(e.event_type.as_str(), "wave" | "task")).collect();
    let agreed: Vec<&SpecEvent> = read.iter().copied().filter(|e| e.block() == Some(Block::Agreed)).collect();

    let execution = execution(context, wave);
    let tree = execution.copy.as_ref().map_or(root, |copy| Path::new(&copy.path));
    let prepared = prepared::for_tasks(root, tree, &of_type("task"));
    let files = wave_files(log, wave);
    // Explicit skills and scoped skills of the touched area. The latter
    // must survive native-tool interception and ignored worktree files.
    let mut named = skills_named(log, wave);
    named.sort();
    // Os arquivos de leitura de cada tarefa: os que ela declara em
    // `must_read`. Um caminho que não existe no projeto (a parte antes do
    // `#`, quando ele aponta uma função) fica de fora; a tarefa sem nenhum
    // arquivo não entra.
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
            .filter(|file| cited_exists(root, map, file));
        task_reads.entry(task.id).or_default().extend(must_read.map(str::to_string));
    }
    let task_reads: Vec<(String, Vec<String>)> = task_reads
        .into_iter()
        .filter(|(_, files)| !files.is_empty())
        .map(|(task, mut files)| {
            files.sort();
            files.dedup();
            let files = files.into_iter().map(|file| if prepared::changed(root, tree, &file) { file } else { with_current_lines(map, file) }).collect();
            (codes.get(&task).cloned().unwrap_or_else(|| task.to_string()), files)
        })
        .collect();
    // Os arquivos de teste que o mapa do projeto conhece para cada arquivo
    // que uma tarefa cita: a linha da tarefa os lista logo abaixo do
    // arquivo, para o agente não sair procurando um por um no código. Um
    // arquivo que o mapa não conhece, ou sem teste externo conhecido, fica
    // de fora — a linha continua como hoje, sem inventar nada.
    let file_tests = task_file_tests(map, &of_type("task"));
    let task_patterns = task_patterns(map, (log, tree), &of_type("task"), &codes, lang);
    // O aviso de arquivo mudado depois do texto sai só na onda que está fora
    // ou sai agora: as outras montagens do pedido não leem o git por tarefa.
    let task_changes = if context.flight.running.contains(&wave) { changed::since_text(root, log, &of_type("task"), &codes) } else { BTreeMap::new() };

    let mut skills = Vec::new();
    let mut bad_skills = Vec::new();
    let mut stale_skills = Vec::new();
    let mut skill_hashes = BTreeMap::new();
    // A tarefa de criar skill nomeia o skill-creator, que mora fora do
    // projeto: ele entra inteiro, sem a conferência das skills do projeto.
    let creator = crate::io::skill_creator::folder();
    let creator_file = creator.as_deref().map(|dir| std::path::absolute(dir.join("SKILL.md")).unwrap_or_else(|_| dir.join("SKILL.md")));
    let mut chosen: Vec<_> =
        named.iter().map(|name| (name.clone(), read_skill(root, &files, name).or_else(|| creator_skill(name, creator_file.as_deref())))).collect();
    // As duas leituras passam por `skills_in`, que dá um arquivo por nome em
    // todas as pastas da onda: o mesmo caminho é a mesma skill.
    for (name, text, file) in scoped_skills(root, &files) {
        if !chosen.iter().any(|(_, source)| source.as_ref().is_some_and(|(_, path)| path == &file)) {
            chosen.push((name, Some((text, file))));
        }
    }
    for (name, source) in &chosen {
        let Some((text, file)) = source else {
            bad_skills.push((name.clone(), MapRefusal::SkillMissingPaths { paths: vec![skill_file(name)] }));
            continue;
        };
        let from_creator = creator_file.as_ref() == Some(file);
        if !from_creator && let Err(refusal) = check_skill(text, |cited| cited_exists(root, map, cited)) {
            bad_skills.push((name.clone(), refusal));
            continue;
        }
        let stale = !from_creator && cited_changed_after(map, text, file);
        if stale {
            stale_skills.push(name.clone());
        }
        let path = shown(file);
        let mut hash = crate::io::sha256::Sha256::new();
        hash.update(text.as_bytes());
        skill_hashes.insert(path.clone(), hash.hex_digest());
        skills.push(Skill { name: name.clone(), path, stale });
    }
    let creator_note = creator.as_deref().zip(creator_file.as_deref()).map(|(folder, file)| {
        let note = crate::platform::i18n::translate("prompt.task.skill_creator", lang)
            .replace("{folder}", &shown(folder))
            .replace("{reserved}", RESERVED_SKILLS)
            .replace("{root}", &shown(root))
            .replace("{spec}", spec);
        (file, note)
    });
    let picks = &context.flight.skill_picks;
    let TaskSkills { blocks: task_skills, ties: skill_ties, candidates: skill_candidates } =
        task_skills(root, log, &of_type("task"), &codes, (&chosen, &skills), creator_note, picks);

    let material = Material {
        spec: spec.to_string(),
        wave,
        block,
        criteria: Vec::new(),
        agreed,
        attended,
        // A linha do conserto que a análise tirou do pedido sai também daqui.
        fix: wave_prompt::fix_lines(log, wave).into_iter().filter(|line| read.iter().any(|e| e.id == line.id)).collect(),
        summary: wave_prompt::summary_of(log, wave),
        // O que a própria onda entregou é o que o revisor confere, e o
        // pedido da onda não o lista.
        own_delivered: Vec::new(),
        execution,
        lessons,
        skills,
        task_reads,
        file_tests,
        changes: Vec::new(),
        validation: Vec::new(),
        since_verdict: Vec::new(),
        chains: BTreeMap::new(),
        codes,
        task_patterns,
        task_changes,
        // Mandatory rules reach the executor despite omitClaudeMd. They
        // are not optional candidates for probabilistic selection.
        project_rules: project_rules(root, &wave_prompt::wave_files(log, wave)),
        prepared,
        task_skills,
    };
    let mut text = wave_prompt::write(&material, lang);
    let observed=crate::io::project_profile::summary(root,&wave_prompt::wave_files(log,wave));
    if observed["available"]==true && observed["data"]["size"]["code_files"].as_u64().is_some_and(|n|n>0) {
        // The complete profile stays in SQLite. Waves pull the scoped details
        // only when needed, rather than repeating every observed convention.
        text.push_str(if lang==Locale::PtBr {"\nPerfil observado do projeto (não é regra obrigatória): "} else {"\nObserved project profile (not a mandatory rule): "});
        text.push_str(&serde_json::json!({"size":observed["data"]["size"],"languages":observed["data"]["languages"]}).to_string());
        text.push_str("\n`mustard-rt run knowledge --profile --file <task-file> --root ");
        text.push('\'');text.push_str(&root.to_string_lossy().replace('\'',"'\\''"));text.push('\'');
        text.push_str("`\n");
    }
    let skills=material.skills.iter().map(|skill| {
            serde_json::json!({"name":skill.name,"path":skill.path,"sha":skill_hashes[&skill.path],
        "stale_at_dispatch":skill.stale,"read_confirmed":null})
        }).collect();
    let lines = wave_prompt::count_lines(&text);
    // O nome do agente, o mesmo em toda onda, de uma tarefa ou de várias: o
    // molde com esse nome mora no projeto, e o envio grava só o nome.
    let agent = "wave".to_string();
    let model = material.execution.requested_model().to_string();
    let effort = material.execution.requested_effort().to_string();
    let listed = wave_prompt::listed(&material);
    let tasks = wave_prompt::task_blocks(&material, lang);
    WavePrompt {
        wave,
        agent,
        model,
        effort,
        escalated_from: material.execution.escalated_from.clone(),
        text,
        lines,
        listed,
        tasks,
        bad_skills,
        stale_skills,
        bad_settings: Vec::new(),
        skills,
        skill_ties,
        skill_candidates,
    }
}

/// Cada skill que a onda escolheu ler, pelo nome: o texto e o arquivo dela,
/// ou nada quando não foi achada.
type Chosen = [(String, Option<(String, PathBuf)>)];

/// As skills de cada tarefa, como a montagem as escolhe ([`task_skills`]).
struct TaskSkills {
    /// As skills de cada tarefa, citadas pelo caminho, pelo código dela.
    blocks: BTreeMap<String, Vec<TaskSkill>>,
    /// As tarefas com mais candidatas do que cabem, ainda sem escolha.
    ties: Vec<SkillTie>,
    /// As candidatas de todas as tarefas, pelo nome, uma vez cada.
    candidates: Vec<String>,
}

/// Quantos caracteres do texto da tarefa e dos itens que ela cobre a escolha
/// das skills mostra.
const TIE_TEXT_CHARS: usize = 2_000;

/// O texto da tarefa `task` e o dos itens que ela cobre, em `log`, cortado em
/// [`TIE_TEXT_CHARS`].
fn tie_text(log: &SpecLog, task: &SpecEvent) -> String {
    let own = task.str_field("text").unwrap_or_default().trim().to_string();
    let covered = task.ints("covers").into_iter().filter_map(|id| log.current(id)).filter_map(|item| {
        let text = item.str_field("text").or_else(|| item.str_field("then")).unwrap_or_default().trim();
        (!text.is_empty()).then(|| format!("- {text}"))
    });
    let joined = std::iter::once(own).chain(covered).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n");
    joined.chars().take(TIE_TEXT_CHARS).collect()
}

/// A candidata `name`, de texto `text` e arquivo `file`, como a escolha a
/// mostra: a descrição inteira e o começo do corpo.
fn skill_option(name: &str, text: &str, file: &Path) -> SkillOption {
    let description = crate::domain::skill::frontmatter::parse(text).map(|front| front.description).unwrap_or_default();
    let body = crate::domain::skill::frontmatter::body(text);
    SkillOption {
        name: name.to_string(),
        path: shown(file),
        description: description.trim().to_string(),
        excerpt: body.trim().chars().take(wave_prompt::SKILL_EXCERPT_CHARS).collect(),
    }
}

/// As skills de cada tarefa, citadas pelo caminho, pelo código dela: a que
/// a tarefa nomeia primeiro, sempre, depois as que casam com os arquivos dela, sem repetir,
/// até [`wave_prompt::TASK_SKILLS_MAX`]. Só entra a skill que a conferência
/// da onda aceitou (`passed`), com o texto que a escolha da onda leu
/// (`chosen`). Quando sobram mais candidatas do que cabem, entram as que a
/// escolha deu para a tarefa em `picks`, na ordem dela; sem escolha, as
/// primeiras, e a tarefa volta entre as que esperam escolha. O skill-creator
/// (`creator`, o arquivo dele e a instrução) vem citado com a instrução.
fn task_skills(
    root: &Path,
    log: &SpecLog,
    tasks: &[&SpecEvent],
    codes: &BTreeMap<u64, String>,
    (chosen, passed): (&Chosen, &[Skill]),
    creator: Option<(&Path, String)>,
    picks: &BTreeMap<u64, Vec<String>>,
) -> TaskSkills {
    let mut out = TaskSkills { blocks: BTreeMap::new(), ties: Vec::new(), candidates: Vec::new() };
    let accepted = |file: &Path| passed.iter().find(|skill| skill.path == shown(file));
    for task in tasks {
        let files: Vec<String> = declared_files(task);
        let named = task.str_field("skill").map(str::trim).filter(|name| !name.is_empty());
        let mut picked: Vec<(String, &str, PathBuf)> = Vec::new();
        if let Some((name, Some((text, file)))) = named.and_then(|named| chosen.iter().find(|(name, _)| name == named))
            && accepted(file).is_some()
        {
            picked.push((name.clone(), text.as_str(), file.clone()));
        }
        let room = wave_prompt::TASK_SKILLS_MAX.saturating_sub(picked.len());
        let mut others: Vec<(String, &str, PathBuf)> = Vec::new();
        // A skill da tarefa é a que a onda escolheu com o mesmo nome: a parte
        // do projeto em que a tarefa mexe pode guardar uma cópia mais velha.
        for (name, ..) in scoped_skills(root, &files) {
            let Some((_, Some((text, file)))) = chosen.iter().find(|(chosen, source)| *chosen == name && source.is_some()) else {
                continue;
            };
            let file = file.clone();
            if accepted(&file).is_some() && !picked.iter().chain(&others).any(|(_, _, had)| *had == file) {
                others.push((name, text.as_str(), file));
            }
        }
        for (name, ..) in picked.iter().chain(&others) {
            if !out.candidates.contains(name) {
                out.candidates.push(name.clone());
            }
        }
        match picks.get(&task.id) {
            Some(pick) => {
                picked.extend(pick.iter().filter_map(|path| others.iter().find(|(_, _, file)| shown(file) == *path).cloned()).take(room));
            }
            None => {
                if others.len() > room {
                    out.ties.push(SkillTie {
                        task: task.id,
                        title: wave_prompt::item_title(task).unwrap_or_default(),
                        text: tie_text(log, task),
                        room,
                        candidates: others.iter().map(|(name, text, file)| skill_option(name, text, file)).collect(),
                    });
                }
                picked.extend(others.into_iter().take(room));
            }
        }
        let skills: Vec<TaskSkill> = picked
            .into_iter()
            .filter_map(|(name, _, file)| {
                let skill = accepted(&file)?;
                let note = creator.as_ref().filter(|(at, _)| *at == file.as_path()).map(|(_, note)| note.clone()).unwrap_or_default();
                Some(TaskSkill { name, path: skill.path.clone(), stale: skill.stale, note })
            })
            .collect();
        if !skills.is_empty() {
            out.blocks.insert(codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string()), skills);
        }
    }
    out
}

/// Os caminhos dos arquivos que a tarefa declara.
fn declared_files(task: &SpecEvent) -> Vec<String> {
    task.fields
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| file.get("path").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// As regras da execução da onda `wave`: os comandos do projeto, as outras
/// ondas em andamento com os arquivos delas e a cópia da onda — a que sai
/// agora ou a gravada no envio da que está em andamento.
fn execution(context: &Context, wave: u64) -> Execution {
    let (log, flight) = (context.log, context.flight);
    let running = flight.running.iter().filter(|n| **n != wave).map(|n| (*n, wave_files(log, *n))).collect();
    let copy = match flight.copies.get(&wave) {
        Some(copy) => Some(copy.clone()),
        None => recorded_copy(log, wave).filter(|_| flight.running.contains(&wave)),
    };
    let host=flight.host.as_deref().or_else(||log.last_by_wave("send").get(&wave).and_then(|id|log.get(*id)).and_then(|send|send.str_field("host")));
    // Codex's native executor inherits its session settings. Claude family
    // names and effort aliases must not become a requested Codex model.
    let mut execution=Execution { running, copy, ..context.base.clone() };
    execution.prepared = execution
        .copy
        .as_ref()
        .and_then(|copy| crate::io::workspace::checkout_git_dir(Path::new(&copy.path)))
        .is_some_and(|admin| admin.join("mustard-dependencies.sha256").is_file());
    if host==Some("codex") {execution.model="inherit".into();execution.effort="inherit".into();}
    else if execution.by_difficulty {
        // O modelo da onda sai da dificuldade das tarefas dela, e sobe um
        // degrau quando ela volta reprovada.
        let chosen = crate::domain::agent_model::wave_model(log, wave);
        execution.model = chosen.model;
        execution.escalated_from = chosen.escalated_from;
    }
    execution
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

/// O skill-creator, quando a tarefa o nomeia (`name`) e o projeto não tem
/// skill com esse nome: o texto e o caminho absoluto do arquivo dele
/// (`file`), achado fora do projeto.
fn creator_skill(name: &str, file: Option<&Path>) -> Option<(String, PathBuf)> {
    let file = file.filter(|_| name == crate::io::skill_creator::NAME)?;
    Some((std::fs::read_to_string(file).ok()?, file.to_path_buf()))
}

/// O caminho de uma skill dentro do subprojeto que a tem.
fn skill_file(name: &str) -> String {
    format!(".claude/skills/{name}/SKILL.md")
}

/// O arquivo de uma skill, procurado nas pastas dos arquivos da onda e, por
/// último, na raiz do projeto: a skill mora no subprojeto em que a tarefa
/// mexe. Devolve o texto, que a conferência lê, e o caminho absoluto do
/// arquivo no projeto principal, que é o que o pedido recomenda: a pasta das
/// skills pode ficar fora do git, e então a cópia da onda não a tem: o agente a
/// abre de primeira pelo projeto principal. A que mora numa pasta do git, como
/// `.agents/skills`, também entra pelo caminho do projeto principal.
fn read_skill(root: &Path, files: &[String], name: &str) -> Option<(String, PathBuf)> {
    skills_in(&skill_folders(root, files)).into_iter().find(|(found, ..)| found == name).map(|(_, text, path)| (text, path))
}

/// As pastas em que um projeto guarda as skills: a do Claude Code e a do
/// Codex. O projeto que usa os dois costuma ter a mesma skill nas duas.
const SKILL_DIRECTORIES: [&str; 2] = [".claude/skills", ".agents/skills"];

/// As skills das pastas `folders` do projeto, uma por nome, em ordem de
/// nome: o nome, o texto e o caminho absoluto do `SKILL.md`. O nome é o
/// `name` do cabeçalho e, sem ele, o da pasta da skill. A mesma skill em mais
/// de um lugar — nas duas pastas de [`SKILL_DIRECTORIES`] ou em duas partes do
/// projeto — entra uma vez só, a do arquivo modificado por último; no empate,
/// a da primeira pasta e, nela, a do Claude Code. Toda leitura de skill do
/// pedido passa por aqui, então o caminho devolvido é a identidade da skill.
fn skills_in(folders: &[PathBuf]) -> Vec<(String, String, PathBuf)> {
    let mut found: BTreeMap<String, (Option<std::time::SystemTime>, String, PathBuf)> = BTreeMap::new();
    for directory in folders.iter().flat_map(|folder| SKILL_DIRECTORIES.map(|directory| folder.join(directory))) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path().join("SKILL.md")).collect();
        paths.sort();
        for path in paths {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let header = crate::domain::skill::frontmatter::parse(&text).ok().and_then(|front| front.name());
            let Some(name) = header.or_else(|| path.parent().and_then(Path::file_name).and_then(|name| name.to_str()).map(str::to_string)) else {
                continue;
            };
            let modified = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok();
            if found.get(&name).is_some_and(|(had, ..)| *had >= modified) {
                continue;
            }
            let absolute = std::path::absolute(&path).unwrap_or(path);
            found.insert(name, (modified, text, absolute));
        }
    }
    found.into_iter().map(|(name, (_, text, path))| (name, text, path)).collect()
}

fn skill_folders(root: &Path, files: &[String]) -> Vec<PathBuf> {
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
    folders
}

fn scoped_skills(root: &Path, files: &[String]) -> Vec<(String, String, PathBuf)> {
    let mut found = Vec::new();
    for (name, text, path) in skills_in(&skill_folders(root, files)) {
        let Ok(front) = crate::domain::skill::frontmatter::parse(&text) else { continue };
        // A manual-only procedure must not run through automatic scope selection.
        if front.extra.get("disable-model-invocation").and_then(Value::as_str)
            .is_some_and(|value| ["true", "yes", "on", "1"].iter().any(|truth| value.eq_ignore_ascii_case(truth)))
            || rules::matches_paths(&text, files) != Some(true) { continue; }
        found.push((name, text, path));
    }
    found
}

/// Um caminho citado (pela skill, ou pela leitura obrigatória de uma tarefa)
/// existe? A conferência olha só a parte antes do `#`: a leitura obrigatória
/// aceita `caminho#função`, e a função não é um arquivo. Pode citar só o fim
/// do caminho, então a busca é pelo que o projeto tem.
fn cited_exists(root: &Path, map: &MapParts<'_>, cited: &str) -> bool {
    let cited = cited.split('#').next().unwrap_or(cited);
    if root.join(cited).exists() {
        return true;
    }
    map.paths().is_some_and(|map| map.modules.iter().any(|m| m.path.ends_with(cited)))
}

/// Um arquivo de leitura obrigatória `caminho#função`, com as linhas atuais
/// da declaração anexadas ao fim (`@início-fim`, uma faixa por trecho,
/// separadas por vírgula, para o nome que se repete no arquivo), achadas no
/// mapa do projeto pelo caminho do módulo e pelo nome da declaração. Sem
/// mapa, sem a função nele ou sem a linha final dela, o arquivo volta sem
/// mudança: [`wave_prompt::WavePrompt::text`] então só manda ler a função
/// pelo nome, como antes. Um caminho sozinho (sem `#`) também volta sem
/// mudança.
fn with_current_lines(map: &MapParts<'_>, file: String) -> String {
    let Some((path, name)) = file.split_once('#') else {
        return file;
    };
    if path.is_empty() || name.is_empty() {
        return file;
    }
    let Some(ranges) = decl_lines(map, path, name) else {
        return file;
    };
    let lines = ranges.iter().map(|(start, end)| format!("{start}-{end}")).collect::<Vec<_>>().join(", ");
    format!("{file}@{lines}")
}

/// Os arquivos de teste que o mapa do projeto conhece para cada arquivo
/// citado pelas tarefas `tasks`, pelo caminho como a tarefa o escreve. Sem
/// mapa, ou para um arquivo que ele não conhece ou sem teste externo
/// conhecido, o arquivo fica de fora.
fn task_file_tests(map: &MapParts<'_>, tasks: &[&SpecEvent]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for task in tasks {
        let paths = task.fields.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        for path in paths.iter().filter_map(|file| file.get("path").and_then(Value::as_str)) {
            if out.contains_key(path) {
                continue;
            }
            if let Some(coverage) = map.tests(path).and_then(|map| tests_for(&map, path).ok())
                && !coverage.files.is_empty()
            {
                out.insert(path.to_string(), coverage.files);
            }
        }
    }
    out
}

/// O padrão do projeto sob cada tarefa de `tasks`, pelo código dela: as
/// regras dos papéis que os arquivos dela tocam, os arquivos dela que passam
/// do corte de tamanho do projeto, a receita do git de cada arquivo dela e
/// os exemplos que seguem as regras fortes. O papel de um arquivo que uma
/// tarefa do plano `log` cria sai do nome dele, como o de um arquivo que já
/// existe. Os exemplos saem dos arquivos da tarefa, um de cada por vez, o
/// melhor de cada primeiro ([`pattern_example`]), e só com regra. O arquivo
/// que a própria tarefa já lista sai da receita. O exemplo cujo arquivo na
/// cópia `tree` não é o que o mapa leu — o commit que a passada em segundo
/// plano ainda não releu — sai sem as linhas. Sem mapa, nada; a tarefa sem
/// regra, sem arquivo grande e sem receita fica de fora.
fn task_patterns(
    map: &MapParts<'_>,
    (log, tree): (&SpecLog, &Path),
    tasks: &[&SpecEvent],
    codes: &BTreeMap<u64, String>,
    lang: Locale,
) -> BTreeMap<String, TaskPattern> {
    let files_of = |task: &SpecEvent| -> Vec<String> {
        let files = task.fields.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        files.iter().filter_map(|file| file.get("path").and_then(Value::as_str)).map(str::to_string).collect()
    };
    let every_task = || log.events.iter().filter(|e| e.event_type == "task").flat_map(files_of).collect();
    let (Some(read), Some(pattern)) = (map.pattern(), map.learned(every_task)) else {
        return BTreeMap::new();
    };
    let cuts = QualityCuts::of(&read.modules);
    let mut out = BTreeMap::new();
    for task in tasks {
        let files = files_of(task);
        let roles: BTreeSet<&str> = files.iter().filter_map(|path| pattern.roles.get(path)).map(String::as_str).collect();
        let mut found = wave_prompt::rules_for(pattern, &roles);
        found.large = files.iter().filter(|path| read.module(path).is_some_and(|m| cuts.large(m))).cloned().collect();
        found.recipes = files
            .iter()
            .filter_map(|file| map.recipe(read, file))
            .filter_map(|recipe| recipe_keeping(recipe, |path| !files.iter().any(|file| file == path)))
            .collect();
        let ruled = !found.strong.is_empty() || !found.info.is_empty();
        if !ruled && found.large.is_empty() && found.recipes.is_empty() {
            continue;
        }
        if ruled {
            let picks: Vec<Vec<String>> = files
                .iter()
                .map(|file| {
                    let known = map.picks.borrow().get(file).cloned();
                    known.unwrap_or_else(|| {
                        let found: Vec<String> = examples_following(read, file, lang, pattern).picks.into_iter().map(|p| p.path).collect();
                        map.picks.borrow_mut().insert(file.clone(), found.clone());
                        found
                    })
                })
                .collect();
            let deepest = picks.iter().map(Vec::len).max().unwrap_or_default();
            let mut seen: BTreeSet<String> = BTreeSet::new();
            for path in (0..deepest).flat_map(|rank| picks.iter().filter_map(move |list| list.get(rank))) {
                if seen.insert(path.clone())
                    && let Some(example) = map.example(read, path)
                {
                    let stale = prepared::changed(map.root, tree, &example.path);
                    found.examples.push(if stale { PatternExample { start: 0, end: 0, ..example } } else { example });
                }
            }
        }
        out.insert(codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string()), found);
    }
    out
}

/// Quantas declarações de um arquivo o exemplo lê, uma a uma, atrás de uma
/// função.
const EXAMPLE_NAMES_READ: usize = 6;

/// O exemplo do padrão no arquivo `path`, com as linhas: a declaração com o
/// nome do arquivo (sem a extensão e sem diferença de caixa, `_` ou `-`);
/// senão, a primeira função ou método entre as primeiras
/// [`EXAMPLE_NAMES_READ`] que ele declara; senão, a primeira delas com
/// linhas. `None` quando nenhuma tem linhas conhecidas.
fn pattern_example(map: &MapParts<'_>, read: &ProjectMap, path: &str) -> Option<PatternExample> {
    let module = read.module(path)?;
    let plain = |text: &str| -> String { text.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase() };
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = plain(file.split('.').next().unwrap_or(file));
    let mut names: Vec<&str> = Vec::new();
    for name in module.declarations.iter().map(|d| d.name.as_str()) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.sort_by_key(|name| plain(name) != stem);
    let mut first: Option<PatternExample> = None;
    for name in names.into_iter().take(EXAMPLE_NAMES_READ) {
        let Some(named) = map.declarations(path, name) else {
            continue;
        };
        let mut found = named.modules.iter().filter(|m| m.path == path).flat_map(|m| &m.declarations);
        let Some(decl) = found.find(|d| d.name == name && d.end_line > 0) else {
            continue;
        };
        let example = PatternExample { name: name.to_string(), path: path.to_string(), start: decl.line, end: decl.end_line };
        if plain(name) == stem || matches!(decl.kind.as_str(), "function" | "method") {
            return Some(example);
        }
        first.get_or_insert(example);
    }
    first
}

/// As faixas de linha (começo, fim) de cada declaração de nome `name` no
/// módulo `path` — mais de uma quando o nome se repete no arquivo. O módulo é
/// o primeiro do mapa com esse caminho, ou terminado nele. `None` quando o
/// mapa não tem o módulo, ou quando nenhuma ocorrência tem a linha final
/// resolvida.
fn decl_lines(map: &MapParts<'_>, path: &str, name: &str) -> Option<Vec<(u64, u64)>> {
    let found = map.paths()?.modules.iter().find(|m| m.path == path || m.path.ends_with(path))?.path.clone();
    let named = map.declarations(&found, name)?;
    let module = named.modules.iter().find(|m| m.path == found)?;
    let ranges: Vec<(u64, u64)> = module.declarations.iter().filter(|d| d.name == name && d.end_line > 0).map(|d| (d.line, d.end_line)).collect();
    (!ranges.is_empty()).then_some(ranges)
}

/// Algum arquivo que a skill cita mudou no git depois do próprio arquivo
/// dela? A skill vale pelo que estava no código quando foi escrita: a data
/// do arquivo dela é a da última escrita, e a de cada arquivo citado é a do
/// commit mais novo que o mudou, pelo histórico do mapa. O citado pode trazer
/// só o fim do caminho, como a conferência aceita; a pasta citada não conta,
/// porque não tem commit próprio. Sem mapa, ou sem a data do arquivo da
/// skill, não há marca.
fn cited_changed_after(map: &MapParts<'_>, text: &str, skill: &Path) -> bool {
    let Some(history) = map.history() else {
        return false;
    };
    let Some(written) = modified_at(skill) else {
        return false;
    };
    cited_paths(text)
        .iter()
        .filter(|cited| !cited.ends_with('/'))
        .flat_map(|cited| {
            let tail = format!("/{cited}");
            history.paths.iter().filter(move |path| *path == cited || path.ends_with(&tail))
        })
        .any(|path| file_history(history, path).is_some_and(|h| h.last_at > written))
}

/// A data da última escrita de um arquivo, em segundos desde 1970, a mesma
/// régua das datas de commit do histórico do mapa.
fn modified_at(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).and_then(|meta| meta.modified()).ok()?;
    let seconds = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(seconds).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{parse_log, render_line, stamp};
    use crate::domain::wave_prompt::Reuse;
    use serde_json::json;
    use tempfile::tempdir;

    fn log_of(events: &[(&str, Value)]) -> SpecLog {
        let mut content = String::new();
        for (i, (event_type, body)) in events.iter().enumerate() {
            let mut map = crate::domain::spec_events::normalize(body.as_object().cloned().unwrap_or_default(), event_type);
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, i as u64 + 1, None, "2026-09-15T10:00:00-03:00")));
            content.push('\n');
        }
        parse_log(&content)
    }

    /// A revisão final lista só o que nenhuma prova verde cobre: a tarefa sem
    /// passo de término, o combinado que nenhuma tarefa terminada cobre — a
    /// resposta `met` da entrega não é prova — e o critério sem execução que
    /// passou. A onda, a entrega, o commit e o recibo da máquina não entram, e
    /// o pedido manda ler tudo numa chamada só.
    #[test]
    fn final_review_lists_only_what_no_green_proof_covers() {
        let log = log_of(&[
            ("rule", json!({"title": "Coberta", "text": "Vale.", "keys": ["a"], "example": "e"})),
            ("decision", json!({"title": "Solta", "text": "Decidido."})),
            ("rule", json!({"title": "Respondida", "text": "Vale também.", "keys": ["b"], "example": "e"})),
            ("criterion", json!({"when": "a", "then": "b", "proof": "echo ok", "form": "ubiquitous"})),
            ("criterion", json!({"when": "c", "then": "d", "form": "ubiquitous"})),
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [4, 5], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Feita", "files": [{"path": "a.rs"}], "covers": [1]})),
            ("task", json!({"wave": 1, "text": "Sem passo", "files": [{"path": "b.rs"}]})),
            ("step", json!({"wave": 1, "item": 7, "text": "Pronta."})),
            ("criterion_run", json!({"criterion": 4, "result": "pass", "exit": 0, "ms": 1})),
            ("delivered", json!({"wave": 1, "text": "Pronta.", "files": ["a.rs"], "agreed": [{"item": 3, "met": true}]})),
            ("stage_run", json!({"phase": "final", "stage": "final-validation", "result": "pass", "ms": 0, "scope": "receipt", "fingerprint": "f"})),
        ]);
        let codes = log.codes();
        let root = tempdir().unwrap();
        let request = final_review(root.path(), "teste", &log, Locale::PtBr);
        assert_eq!(request.listed, [codes[&8].clone(), codes[&2].clone(), codes[&3].clone(), codes[&5].clone()], "{}", request.text);
        assert!(request.text.contains("dispatch-review"), "{}", request.text);
    }

    fn plan_log() -> SpecLog {
        log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}], "skill": "somar"})),
        ])
    }

    /// A montagem faz só os pedidos das ondas pedidas, e a rodada pede só as
    /// que saem agora: a onda planejada que ninguém pediu não é montada.
    #[test]
    fn only_the_requested_waves_are_assembled() {
        let dir = tempdir().unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}]})),
            ("wave", json!({"n": 2, "text": "Outra onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 2, "text": "Subtrair", "files": [{"path": "apps/rt/src/b.rs"}]})),
        ]);
        let built = prompts(dir.path(), "teste", &log, Locale::PtBr, &Flight::default(), &BTreeSet::from([2]));
        assert_eq!(built.iter().map(|p| p.wave).collect::<Vec<_>>(), vec![2]);
    }

    /// The wave of two graded tasks goes on the largest model of them, at
    /// effort xhigh whatever the configuration says, and the request names
    /// them. Turned off in the configuration, it goes on the configured
    /// model; on Codex, it inherits. Rejected by a verdict after its send, it
    /// goes one step up and says where it came from.
    #[test]
    fn the_wave_goes_on_the_model_of_its_hardest_task_at_effort_xhigh() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"agents":{"model":"opus","effort":"low"}}"#).unwrap();
        let graded = |level: u64| json!({"name": "Soma", "position": 1, "formed": true, "difficulty": {"level": level, "confidence": 0.9}});
        let plan = [
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}], "group": graded(0)})),
            ("task", json!({"wave": 1, "text": "Subtrair", "files": [{"path": "apps/rt/src/b.rs"}], "group": graded(1)})),
        ];
        let claude = Flight { host: Some("claude".into()), ..Flight::default() };
        let built = |log: &SpecLog, flight: &Flight| prompts(root, "teste", log, Locale::PtBr, flight, &BTreeSet::from([1])).remove(0);
        let first = built(&log_of(&plan), &claude);
        assert_eq!((first.model.as_str(), first.effort.as_str(), first.escalated_from.clone()), ("sonnet", "xhigh", None));
        assert!(first.text.contains("Modelo desta onda: sonnet. Esforço: xhigh."), "{}", first.text);
        let codex = built(&log_of(&plan), &Flight { host: Some("codex".into()), ..Flight::default() });
        assert_eq!((codex.model.as_str(), codex.effort.as_str()), ("inherit", "inherit"));
        let ungraded = [plan[0].clone(), ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}]}))];
        assert_eq!(built(&log_of(&ungraded), &claude).model, "sonnet", "a task without a grade goes to Sonnet, not to the configured Opus");

        let mut rejected = plan.to_vec();
        rejected.push(("send", json!({"wave": 1, "role": "wave", "text": "pedido", "lines": 1, "chars": 6, "mustard": "0", "model": "sonnet"})));
        rejected.push(("verdict", json!({"wave": 1, "result": "rejected", "text": "faltou o teste", "author": "review"})));
        let fix = built(&log_of(&rejected), &claude);
        assert_eq!(fix.model, "opus", "{}", fix.text);
        assert_eq!(fix.escalated_from, Some(crate::domain::agent_model::Escalation { from: "sonnet".into(), cause: 5 }));

        std::fs::write(root.join("mustard.json"), r#"{"agents":{"model":"opus","by_difficulty":false}}"#).unwrap();
        let off = built(&log_of(&rejected), &claude);
        assert_eq!((off.model.as_str(), off.effort.as_str(), off.escalated_from), ("opus", "xhigh", None), "turned off, the configured model");
    }

    fn write_skill(root: &Path, subproject: &str, name: &str, text: &str) {
        let dir = root.join(subproject).join(".claude").join("skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), text).unwrap();
    }

    /// O mapa de teste, gravado pela porta do mapa, como o scan o grava.
    fn write_map(root: &Path, model: &Value) {
        crate::io::project_map::write_text(root, &model.to_string()).unwrap();
    }

    /// O caminho do arquivo da skill como o pedido o escreve: absoluto, no
    /// projeto principal `root`, com barras normais.
    fn skill_path_in(root: &Path, subproject: &str, name: &str) -> String {
        shown(&root.join(subproject).join(".claude").join("skills").join(name).join("SKILL.md"))
    }

    /// A skill que a tarefa nomeia entra no pedido pelo caminho absoluto do
    /// arquivo no projeto principal, que existe mesmo com a onda saindo numa
    /// cópia fora dele, sem a descrição nem o corpo; e ela é procurada no
    /// subprojeto em que a tarefa mexe. O caminho nunca é o
    /// da cópia: a pasta das skills fica fora do git e não chega lá.
    #[test]
    fn the_skill_reaches_the_request_by_the_absolute_path_in_the_main_project() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let elsewhere = tempdir().unwrap();
        let copy = shown(&elsewhere.path().join("copia"));
        write_skill(root, "apps/rt", "somar", "---\nname: somar\ndescription: Use ao somar dois números no motor.\n---\n\n# Somar\n\nUm passo por linha.\n");
        let chosen = WaveCopy { path: copy.clone(), reused: None };
        let flight = Flight { running: [1].into(), copies: [(1, chosen)].into(), ..Flight::default() };
        let built = prompts(root, "teste", &plan_log(), Locale::PtBr, &flight, &plan_log().planned_waves());
        assert_eq!(built.len(), 1);
        assert!(built[0].bad_skills.is_empty(), "{:?}", built[0].bad_skills);
        let path = skill_path_in(root, "apps/rt", "somar");
        assert!(Path::new(&path).is_absolute() && Path::new(&path).is_file(), "{path}");
        assert!(built[0].text.contains(&format!("`{path}`")), "{}", built[0].text);
        assert!(!built[0].text.contains("`apps/rt/.claude/skills/somar/SKILL.md`"), "{}", built[0].text);
        assert!(!built[0].text.contains(&format!("{copy}/apps/rt/.claude")), "{}", built[0].text);
        assert!(!built[0].text.contains("Um passo por linha."), "the body stays in its file: {}", built[0].text);
        assert!(!built[0].text.contains("Use ao somar dois números no motor."), "the header stays out: {}", built[0].text);
    }

    /// A skill sem frontmatter entra mesmo assim: o bloco da tarefa a cita
    /// pelo nome e pelo caminho.
    #[test]
    fn a_skill_without_frontmatter_still_reaches_the_request_by_path() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_skill(root, "apps/rt", "somar", "# Somar\n\nUm passo por linha.\n");
        let built = prompts(root, "teste", &plan_log(), Locale::PtBr, &Flight::default(), &plan_log().planned_waves());
        assert!(built[0].bad_skills.is_empty(), "{:?}", built[0].bad_skills);
        let path = skill_path_in(root, "apps/rt", "somar");
        assert!(built[0].text.contains(&format!("- Skill somar, de `{path}`. ")), "{}", built[0].text);
    }

    /// A skill que cita um caminho que não existe, e a que passa do tamanho
    /// máximo, são recusadas, e o texto delas não entra no pedido.
    #[test]
    fn a_skill_citing_a_missing_path_or_too_long_is_refused() {
        let long = format!("# O molde\n\n{}", "linha do molde\n".repeat(600));
        for (text, reason) in [("# O molde\n\nVeja `apps/rt/src/nao-existe.rs`.\n", "skill-missing-path"), (long.as_str(), "skill-too-long")] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            write_skill(root, "apps/rt", "somar", text);
            let built = prompts(root, "teste", &plan_log(), Locale::PtBr, &Flight::default(), &plan_log().planned_waves());
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
        let built = prompts(root, "teste", &plan_log(), Locale::PtBr, &Flight::default(), &plan_log().planned_waves());
        assert!(built[0].text.contains("skills/somar/SKILL.md"), "{}", built[0].text);
        assert!(!built[0].text.contains("skills/subtrair/SKILL.md"), "{}", built[0].text);
        assert!(!built[0].text.contains("MOLDS FOR THIS WAVE"), "{}", built[0].text);
    }

    #[test]
    fn scoped_skills_reach_the_wave_without_a_task_name_and_without_unrelated_procedures() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for (area, name, scope, disabled) in [
            ("", "root-contract", "apps/rt/**", false),
            ("apps/rt", "area-contract", "apps/rt/src/**", false),
            ("apps/rt", "other-contract", "apps/cli/**", false),
            ("apps/rt", "manual-procedure", "apps/rt/**", true),
        ] {
            write_skill(root, area, name, &format!("---\nname: {name}\ndescription: Current area contract.\npaths:\n  - {scope}\ndisable-model-invocation: {disabled}\nsource: manual\n---\n\nOnly read this body when needed.\n"));
        }
        let log = log_of(&[
            ("wave", json!({"n":1,"text":"A onda","criteria":[],"done_when":"passa"})),
            ("task", json!({"wave":1,"text":"Somar","files":[{"path":"apps/rt/src/a.rs"}]})),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        for name in ["root-contract", "area-contract"] {
            assert!(built[0].text.contains(&format!("skills/{name}/SKILL.md")), "{}", built[0].text);
        }
        for hidden in ["other-contract", "manual-procedure"] {
            assert!(!built[0].text.contains(hidden), "{}", built[0].text);
        }
        assert!(!built[0].text.contains("Only read this body when needed."), "the scoped skill is cited, not pasted: {}", built[0].text);
        assert!(built[0].bad_skills.is_empty());
    }

    /// A origem que o cabeçalho da skill declara não a tira da onda: a que a
    /// tarefa nomeia e a que casa com os arquivos dela chegam citadas, mesmo
    /// com a marca dos procedimentos por área que saíram do Mustard.
    #[test]
    fn a_skill_reaches_the_wave_whatever_source_its_header_names() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let header = "metadata: {\"source\":\"mustard-expertise\"}";
        write_skill(root, "apps/rt", "somar", &format!("---\nname: somar\ndescription: Use ao somar.\n{header}\n---\n\n# Somar\n\nUm passo por linha.\n"));
        write_skill(root, "", "contrato", &format!("---\nname: contrato\ndescription: Contrato da área.\npaths:\n  - apps/rt/**\n{header}\n---\n\nO corpo do contrato.\n"));
        let log = log_of(&[
            ("wave", json!({"n":1,"text":"A onda","criteria":[],"done_when":"passa"})),
            ("task", json!({"wave":1,"text":"Somar","skill":"somar","files":[{"path":"apps/rt/src/a.rs"}]})),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        assert!(built[0].bad_skills.is_empty(), "{:?}", built[0].bad_skills);
        assert!(built[0].text.contains("skills/somar/SKILL.md"), "{}", built[0].text);
        assert!(built[0].text.contains("skills/contrato/SKILL.md"), "{}", built[0].text);
    }

    /// O bloco de uma tarefa cita no máximo três skills: a que a tarefa
    /// nomeia primeiro, depois as que casam com os arquivos dela, na ordem em
    /// que a escolha as acha. Nem o corpo nem o cabeçalho entram.
    #[test]
    fn a_task_block_cites_at_most_three_skills_the_named_one_first() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("apps/rt/src")).unwrap();
        std::fs::write(root.join("apps/rt/src/a.rs"), "fn um() {}\n").unwrap();
        for name in ["b-scoped", "c-scoped", "d-scoped"] {
            write_skill(root, "apps/rt", name, &format!("---\nname: {name}\ndescription: Area contract.\npaths:\n  - apps/rt/src/**\nsource: manual\n---\n\nBody of {name}.\n"));
        }
        write_skill(root, "apps/rt", "somar", "---\nname: somar\ndescription: The named one.\n---\n\nBody of somar.\n");
        let built = prompts(root, "teste", &plan_log(), Locale::PtBr, &Flight::default(), &plan_log().planned_waves()).remove(0);
        let block = &built.tasks[0].text;
        let at = |name: &str| block.find(&format!("skills/{name}/SKILL.md")).unwrap_or_else(|| panic!("{name}: {block}"));
        assert!(at("somar") < at("b-scoped") && at("b-scoped") < at("c-scoped"), "{block}");
        assert!(!block.contains("d-scoped") && !block.contains("Body of") && !block.contains("description: The named one."), "{block}");
        assert!(built.text.contains(block.as_str()), "the request carries the first block: {}", built.text);
    }

    /// A tarefa que cria uma skill cita o processo do skill-creator onde a
    /// máquina o tem, com a pasta dos scripts dele e a ordem de segui-lo sem
    /// pedir aprovação a ninguém; o corpo do processo fica no arquivo dele.
    #[test]
    fn a_skill_creation_task_block_cites_the_skill_creator_and_brings_the_instruction() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("apps/rt/src")).unwrap();
        std::fs::write(root.join("apps/rt/src/a.rs"), "fn um() {}\n").unwrap();
        write_skill(root, "apps/rt", "area", "---\nname: area\ndescription: Area contract.\npaths:\n  - apps/rt/src/**\n---\n\nBody of area.\n");
        let installed = tempdir().unwrap();
        let creator = installed.path().join("skill-creator");
        std::fs::create_dir_all(creator.join("scripts")).unwrap();
        std::fs::write(creator.join("SKILL.md"), "---\nname: skill-creator\ndescription: Create skills.\n---\n\n# Skill Creator\n\nRun `scripts/run_eval.py`.\n").unwrap();
        let log = log_of(&[
            ("wave", json!({"n":1,"text":"A onda","criteria":[],"done_when":"passa"})),
            ("task", json!({"wave":1,"title":"Criar a skill: falta o teste","text":"Criar","agent":"As ocorrências.","skill":"skill-creator","files":[{"path":"apps/rt/src/a.rs"}]})),
        ]);
        crate::io::skill_creator::folder_in_test(Some(creator.clone()));
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves()).remove(0);
        assert!(built.bad_skills.is_empty(), "the procedure from outside the project is no bad skill: {:?}", built.bad_skills);
        let block = &built.tasks[0].text;
        assert!(!block.contains("# Skill Creator") && block.contains(&format!("`{}`", shown(&creator.join("SKILL.md")))), "{block}");
        assert!(block.contains(&format!("`{}`", shown(&creator))), "the folder of its scripts: {block}");
        assert!(block.contains("sem pedir aprovação") && block.contains("run write skill"), "{block}");
        assert!(block.contains("`.mustard-skill/<nome>/SKILL.md`"), "the new skill goes to the reserved folder of the copy: {block}");
        assert!(!block.contains(&shown(&root.join(".claude").join("skills"))), "never to the skills folder of the main project: {block}");
        assert!(block.find("skill-creator").unwrap() < block.find("area").unwrap() && !block.contains("Body of area."), "{block}");
        crate::io::skill_creator::folder_in_test(None);
    }

    /// Grava `SKILL.md` em `<directory>/<folder>` com a hora de modificação
    /// dada, e devolve o caminho como o pedido o escreve.
    fn write_skill_at(root: &Path, directory: &str, folder: &str, text: &str, modified: std::time::SystemTime) -> String {
        let dir = root.join(directory).join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("SKILL.md");
        std::fs::write(&file, text).unwrap();
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(modified).unwrap();
        shown(&file)
    }

    /// A mesma skill no `.claude/skills` e no `.agents/skills` do mesmo
    /// subprojeto entra no pedido uma vez só, e é a do arquivo mudado por
    /// último — ora a do Claude Code, ora a do Codex —, nomeada pela tarefa
    /// ou achada pelos arquivos dela.
    #[test]
    fn the_same_skill_in_both_skill_folders_reaches_the_wave_once_the_newest_copy() {
        let now = std::time::SystemTime::now();
        let before = now - std::time::Duration::from_secs(3600);
        for named in [true, false] {
            for newest in ["apps/rt/.claude/skills", "apps/rt/.agents/skills"] {
                let dir = tempdir().unwrap();
                let root = dir.path();
                let mut paths = BTreeMap::new();
                for directory in ["apps/rt/.claude/skills", "apps/rt/.agents/skills"] {
                    let body = format!("Body from {directory}.");
                    let text = format!("---\nname: somar\ndescription: Use ao somar.\npaths:\n  - apps/rt/src/**\n---\n\n{body}\n");
                    let when = if directory == newest { now } else { before };
                    paths.insert(directory, (write_skill_at(root, directory, "somar", &text, when), body));
                }
                let mut task = json!({"wave":1,"text":"Somar","files":[{"path":"apps/rt/src/a.rs"}]});
                if named {
                    task["skill"] = json!("somar");
                }
                let log = log_of(&[("wave", json!({"n":1,"text":"A onda","criteria":[],"done_when":"passa"})), ("task", task)]);
                let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves()).remove(0);
                let block = &built.tasks[0].text;
                let (kept, _) = &paths[newest];
                let context = format!("named={named} newest={newest}: {block}");
                assert_eq!(built.skills.len(), 1, "{context}");
                assert_eq!(built.skills[0]["path"], json!(kept), "{context}");
                assert_eq!(block.matches(kept.as_str()).count(), 1, "{context}");
                assert_eq!(block.matches("- Skill somar,").count(), 1, "{context}");
                for (directory, (path, body)) in &paths {
                    if *directory != newest {
                        assert!(!block.contains(body.as_str()) && !built.text.contains(path.as_str()), "{context}");
                    }
                }
            }
        }
    }

    /// A mesma skill em duas partes do projeto, cada uma com a própria pasta
    /// de skills, entra no pedido uma vez só, a do arquivo mudado por último:
    /// nomeada pela tarefa ou achada pelos arquivos dela, numa tarefa que
    /// mexe nas duas partes ou em duas tarefas, uma em cada parte.
    #[test]
    fn the_same_skill_in_two_project_parts_reaches_the_wave_once_the_newest_copy() {
        let now = std::time::SystemTime::now();
        let before = now - std::time::Duration::from_secs(3600);
        let parts = ["web/.claude/skills", "api/.agents/skills"];
        for named in [true, false] {
            for split in [false, true] {
                for newest in parts {
                    let dir = tempdir().unwrap();
                    let root = dir.path();
                    let mut paths = BTreeMap::new();
                    for directory in parts {
                        let body = format!("Body from {directory}.");
                        let text = format!("---\nname: somar\ndescription: Use ao somar.\npaths:\n  - web/src/**\n  - api/src/**\n---\n\n{body}\n");
                        let when = if directory == newest { now } else { before };
                        paths.insert(directory, (write_skill_at(root, directory, "somar", &text, when), body));
                    }
                    let task = |files: &[&str]| {
                        let mut task = json!({"wave":1,"text":"Somar","files":files.iter().map(|path| json!({"path":path})).collect::<Vec<_>>()});
                        if named {
                            task["skill"] = json!("somar");
                        }
                        ("task", task)
                    };
                    let mut events = vec![("wave", json!({"n":1,"text":"A onda","criteria":[],"done_when":"passa"}))];
                    if split {
                        events.extend([task(&["web/src/a.ts"]), task(&["api/src/b.rs"])]);
                    } else {
                        events.push(task(&["web/src/a.ts", "api/src/b.rs"]));
                    }
                    let log = log_of(&events);
                    let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves()).remove(0);
                    let (kept, _) = &paths[newest];
                    let context = format!("named={named} split={split} newest={newest}: {}", built.text);
                    assert_eq!(built.skills.len(), 1, "{context}");
                    assert_eq!(built.skills[0]["path"], json!(kept), "{context}");
                    for block in built.tasks.iter().map(|task| &task.text) {
                        assert_eq!(block.matches(kept.as_str()).count(), 1, "{context}");
                        assert_eq!(block.matches("- Skill somar,").count(), 1, "{context}");
                    }
                    for (directory, (path, body)) in &paths {
                        if *directory != newest {
                            assert!(!built.text.contains(body.as_str()) && !built.text.contains(path.as_str()), "{context}");
                        }
                    }
                }
            }
        }
    }

    /// Skills de nomes diferentes, uma em cada pasta, entram as duas.
    #[test]
    fn skills_with_different_names_in_both_folders_both_reach_the_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let now = std::time::SystemTime::now();
        for (directory, name) in [("apps/rt/.claude/skills", "somar"), ("apps/rt/.agents/skills", "subtrair")] {
            let text = format!("---\nname: {name}\ndescription: Area contract.\npaths:\n  - apps/rt/src/**\n---\n\nBody of {name}.\n");
            write_skill_at(root, directory, name, &text, now);
        }
        let log = log_of(&[
            ("wave", json!({"n":1,"text":"A onda","criteria":[],"done_when":"passa"})),
            ("task", json!({"wave":1,"text":"Somar","files":[{"path":"apps/rt/src/a.rs"}]})),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves()).remove(0);
        let block = &built.tasks[0].text;
        assert_eq!(built.skills.len(), 2, "{block}");
        assert!(block.contains("skills/somar/SKILL.md") && block.contains("skills/subtrair/SKILL.md"), "{block}");
    }

    /// O nome da skill é o `name` do cabeçalho, não o da pasta: a mesma
    /// skill guardada sob outro nome de pasta no `.agents/skills` ainda é a
    /// mesma, entra uma vez e com o nome do cabeçalho.
    #[test]
    fn the_skill_name_comes_from_the_header_when_the_folder_has_another_name() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let now = std::time::SystemTime::now();
        let before = now - std::time::Duration::from_secs(3600);
        let text = |body: &str| format!("---\nname: somar\ndescription: Use ao somar.\npaths:\n  - apps/rt/src/**\n---\n\n{body}\n");
        let old = write_skill_at(root, "apps/rt/.claude/skills", "somar", &text("Older body."), before);
        let new = write_skill_at(root, "apps/rt/.agents/skills", "somar-codex", &text("Newer body."), now);
        let built = prompts(root, "teste", &plan_log(), Locale::PtBr, &Flight::default(), &plan_log().planned_waves()).remove(0);
        let block = &built.tasks[0].text;
        assert_eq!(built.skills.len(), 1, "{block}");
        assert_eq!(built.skills[0]["name"], json!("somar"), "{block}");
        assert_eq!(built.skills[0]["path"], json!(new), "{block}");
        assert!(block.contains(&format!("- Skill somar, de `{new}`")), "{block}");
        assert!(!block.contains("Older body.") && !built.text.contains(&old) && !block.contains("somar-codex,"), "{block}");
    }

    /// As skills de desenvolvimento deste repositório, como estão no git,
    /// chegam ao pedido pelas pastas que declaram: a onda que mexe num gancho
    /// recebe as duas de gancho, a que mexe num comando recebe a de comando,
    /// e a que mexe no scan não recebe nenhuma — sem a tarefa nomear skill.
    #[test]
    fn the_repository_skills_reach_the_wave_by_the_folders_they_declare() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = tempdir().unwrap();
        let root = dir.path();
        let names = ["add-hook-rule", "add-run-command", "refazer-gancho"];
        for name in names {
            let text = std::fs::read_to_string(repository.join("apps/rt/.agents/skills").join(name).join("SKILL.md")).unwrap();
            for cited in cited_paths(&text) {
                let cited = root.join(cited);
                std::fs::create_dir_all(cited.parent().unwrap()).unwrap();
                std::fs::write(cited, "").unwrap();
            }
            let folder = root.join("apps/rt/.agents/skills").join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("SKILL.md"), text).unwrap();
        }
        let log = log_of(&[
            ("wave", json!({"n":1,"text":"Gancho","criteria":[],"done_when":"passa"})),
            ("task", json!({"wave":1,"text":"Mexer no gancho","files":[{"path":"apps/rt/src/hooks/task/end_of_turn_check.rs"}]})),
            ("wave", json!({"n":2,"text":"Comando","criteria":[],"done_when":"passa"})),
            ("task", json!({"wave":2,"text":"Mexer no comando","files":[{"path":"apps/rt/src/commands/flow/plan.rs"}]})),
            ("wave", json!({"n":3,"text":"Scan","criteria":[],"done_when":"passa"})),
            ("task", json!({"wave":3,"text":"Mexer no scan","files":[{"path":"apps/scan/src/main.rs"}]})),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let received = |wave: u64| -> Vec<&str> {
            let prompt = built.iter().find(|prompt| prompt.wave == wave).unwrap();
            assert!(prompt.bad_skills.is_empty(), "{:?}", prompt.bad_skills);
            names.into_iter().filter(|name| prompt.text.contains(&format!(".agents/skills/{name}/SKILL.md"))).collect()
        };
        assert_eq!(received(1), ["add-hook-rule", "refazer-gancho"]);
        assert_eq!(received(2), ["add-run-command"]);
        assert!(received(3).is_empty());
    }

    /// Um item revisto entra na lista do pedido só na versão nova, com o
    /// título dela; a linha da antiga fica fora, e nem o número nem o texto
    /// dela aparecem.
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
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let obey = section_lines(&built[0].text, crate::platform::i18n::translate("prompt.part.obey", Locale::PtBr));
        assert!(obey.contains(&"Limite MSTD-LIMIT-0001 — O pedido cabe em 500 linhas."), "{}", built[0].text);
        assert_eq!(built[0].text.matches("MSTD-LIMIT-0001").count(), 1, "{}", built[0].text);
        assert!(!built[0].text.contains("400 linhas"), "a versão antiga não entra: {}", built[0].text);
    }

    /// A escolha gravada antes de um item combinado ser revisto ainda cita a
    /// versão antiga; o pedido lista só a nova, e a antiga não entra na lista
    /// de leitura por mais que a escolha a tenha posto.
    #[test]
    fn a_choice_that_added_the_old_version_of_a_revised_item_lists_only_the_new_one() {
        let log = log_of(&[
            ("limit", json!({"text": "O pedido cabe em 400 linhas.", "value": "400 linhas", "keys": ["pedido"]})),
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Cortar o pedido", "files": [{"path": "apps/rt/src/a.rs"}]})),
            ("limit", json!({"text": "O pedido cabe em 500 linhas.", "value": "500 linhas", "keys": ["pedido"], "replaces": 1})),
        ]);
        let choice = Choice {
            added: vec![(1, String::from("a regra do pedido serve à onda")), (4, String::from("a regra do pedido serve à onda"))],
            ..Choice::default()
        };
        let languages = Languages::new(["pt-BR", "en-US"]);
        let got = request_items(&log, None, 1, Some(&choice), &languages);
        let ids: Vec<u64> = got.items.iter().map(|e| e.id).collect();
        assert!(ids.contains(&4), "a versão nova entra: {ids:?}");
        assert!(!ids.contains(&1), "a versão antiga não entra: {ids:?}");
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
            (
                "task",
                json!({"wave": 1, "text": "Somar",
                            "files": [{"path": "apps/rt/src/a.rs"}, {"path": "apps/rt/src/b.rs"}]}),
            ),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        assert!(built[0].text.contains("\n- Quem testa `apps/rt/src/a.rs`: `apps/rt/tests/a_test.rs`"), "{}", built[0].text);
        assert!(!built[0].text.contains("Quem testa `apps/rt/src/b.rs`"), "{}", built[0].text);
    }

    /// A leitura obrigatória de uma tarefa que aponta uma função
    /// (`caminho#função`) faz a linha da tarefa dizer o que ler antes, e
    /// manda ler só aquela função, não o arquivo inteiro. As frases de achar
    /// e ler o código pelo mapa, rodar só os testes do que mudou e deixar a
    /// suíte inteira e o lint para a rodada saíram do catálogo — moraram para
    /// o molde do agente — e não voltam a aparecer no pedido.
    #[test]
    fn the_wave_request_asks_to_read_by_excerpt() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("apps/rt/src")).unwrap();
        std::fs::write(root.join("apps/rt/src/a.rs"), "fn soma() {}\n").unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            (
                "task",
                json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}],
                            "must_read": ["apps/rt/src/a.rs#soma"]}),
            ),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        assert!(built[0].text.contains("\n- Leia antes: leia só `soma` em `apps/rt/src/a.rs`"), "{}", built[0].text);
        assert!(!built[0].text.contains("- Leia antes: `apps/rt/src/a.rs#soma`"), "{}", built[0].text);
        assert!(built[0].text.contains("map-source-unverified"), "an unverified map must request a directed read");
        for phrase in ["Ache e leia o código pelo mapa", "só os testes do que mudou", "A suíte inteira e o lint são da rodada"] {
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
            (
                "task",
                json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}],
                            "must_read": ["apps/rt/src/a.rs#soma"]}),
            ),
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
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        assert!(built[0].text.contains("leia só as linhas 3-5 de `soma` em `apps/rt/src/a.rs`"), "{}", built[0].text);
        assert!(!built[0].text.contains("leia só `soma` em `apps/rt/src/a.rs`"), "o texto sem linha não deve sobrar: {}", built[0].text);

        // Um commit soma dez linhas antes da função: ela se desloca, e o
        // mapa gravou as posições novas. O pedido seguinte traz as linhas
        // novas, lidas de novo do disco — não as da montagem anterior.
        write_model(13, 15);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        assert!(built[0].text.contains("leia só as linhas 13-15 de `soma` em `apps/rt/src/a.rs`"), "{}", built[0].text);
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
            (
                "task",
                json!({"wave": 1, "text": "Ajustar", "files": [{"path": "apps/rt/src/a.rs"}],
                            "must_read": ["apps/rt/src/a.rs#Coisa"]}),
            ),
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

        let pt = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let pt_line = pt[0].text.lines().find(|l| l.contains("Coisa")).unwrap_or_default();
        assert!(pt_line.ends_with("leia só as linhas 1-1 de `Coisa` em `apps/rt/src/a.rs`"), "{pt_line}");
        assert!(!pt_line.contains("função"), "{pt_line}");

        let en = prompts(root, "teste", &log, Locale::EnUs, &Flight::default(), &log.planned_waves());
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
            (
                "task",
                json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}],
                            "must_read": ["apps/rt/src/nao-existe.rs"]}),
            ),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let read_before = crate::platform::i18n::translate("prompt.task.read_before", Locale::PtBr);
        assert!(!built[0].text.contains(read_before), "{}", built[0].text);
    }

    /// A seção de como ler, no pedido de uma onda montado como a rodada e o
    /// despacho o montam: os dois comandos — o que lê todos os itens e lições
    /// do pedido numa chamada só e o que relê um item pelo código —, com a
    /// spec e, quando a onda tem cópia, o caminho do repositório principal, e
    /// o aviso de que a entrega sem a leitura completa é recusada. Ela vem
    /// antes de "O que fazer". Sem cópia, o agente roda no repositório
    /// principal, e o caminho sai dos dois comandos. O pedido da revisão final
    /// traz a mesma seção, com a leitura em lote do pedido dele e o aviso da
    /// recusa do veredito.
    #[test]
    fn the_wave_request_tells_how_to_read_each_item_and_warns_of_the_refusal() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let main = shown(root);
        let mut events = Vec::new();
        for n in 1..=3 {
            events.push(("wave", json!({"n": n, "text": "Uma onda", "criteria": [], "done_when": "passa"})));
            events.push(("task", json!({"wave": n, "text": "Somar", "files": [{"path": format!("src/{n}.rs")}]})));
        }
        let log = log_of(&events);
        let copy = WaveCopy { path: "/c/tres".into(), reused: None };
        let in_copy = Flight { running: [3].into(), copies: [(3, copy)].into(), ..Flight::default() };
        for (lang, heading, do_heading, read_all, read_one, warning, gone, final_line) in [
            (
                Locale::PtBr,
                "## Como ler cada item",
                "## O que fazer",
                "- os itens e lições já liberados deste pedido: `mustard-rt run read dispatch-3 {root}--spec teste`",
                "- um item só, para reler: `mustard-rt run read item-<código> {root}--spec teste`",
                "A entrega é recusada se algum item liberado não foi lido. A recusa diz qual.",
                "Antes de começar",
                (
                    "- todos os itens deste pedido: `mustard-rt run read dispatch-review {root}--spec teste`",
                    "O veredito é recusado se algum item deste pedido não foi lido. A recusa diz qual.",
                ),
            ),
            (
                Locale::EnUs,
                "## How to read each item",
                "## What to do",
                "- the items and lessons of this request released so far: `mustard-rt run read dispatch-3 {root}--spec teste`",
                "- a single item, to reread it: `mustard-rt run read item-<item-code> {root}--spec teste`",
                "The delivery is refused if any released item was not read. The refusal says which.",
                "Before you start",
                (
                    "- every item of this request: `mustard-rt run read dispatch-review {root}--spec teste`",
                    "The verdict is refused if any item of this request was not read. The refusal says which.",
                ),
            ),
        ] {
            for (flight, flag) in [(&in_copy, format!("--root {main} ")), (&Flight::default(), String::new())] {
                let built = prompts(root, "teste", &log, lang, flight, &log.planned_waves());
                let wave = &built.iter().find(|p| p.wave == 3).expect("the third wave's request").text;
                let at = wave.find(heading).unwrap_or_else(|| panic!("{lang:?}: {wave}"));
                let to_do = wave.find(do_heading).unwrap_or_else(|| panic!("{lang:?}: {wave}"));
                assert!(at < to_do, "the reading comes before what to do: {wave}");
                let section = &wave[at..to_do];
                for line in [read_all, read_one, warning] {
                    let line = line.replace("{root}", &flag);
                    assert!(section.contains(&line), "{flag:?} {lang:?}: {line}\n{wave}");
                }
                assert!(wave.contains(&format!("read dispatch-3 {flag}--spec teste")), "every item is read in one call: {wave}");
                assert_eq!(wave.matches("--term <").count(), 0, "the batch reads the lessons too: {wave}");
                // The two read commands, the step and delivery commands and the
                // line that says to run them inside the copy.
                assert_eq!(wave.matches("--root").count(), if flag.is_empty() { 0 } else { 5 }, "{wave}");
                assert!(!wave.contains(gone), "reading everything first is gone: {wave}");
            }
            let last = final_review(root, "teste", &log, lang).text;
            let (final_item, final_warning) = final_line;
            let flag = format!("--root {main} ");
            for line in [final_item.replace("{root}", &flag), read_one.replace("{root}", &flag), final_warning.to_string()] {
                assert!(last.contains(&line), "{lang:?}: {line}\n{last}");
            }
            assert!(last.contains(heading) && !last.contains(warning), "the verdict, not the delivery, is refused: {last}");
            assert!(last.contains(&format!("read dispatch-review {flag}--spec teste")), "the final review is read in one call: {last}");
        }
    }

    /// O pedido da onda e o da revisão final dizem no cabeçalho os dois
    /// idiomas lidos do `mustard.json`: o do texto e o dos nomes no código.
    /// O projeto que declara o código em português recebe os nomes em
    /// português, mesmo com o texto em inglês; o que não declara o do código
    /// recebe os nomes em inglês. A linha sai uma vez só em cada pedido.
    #[test]
    fn every_request_opens_with_the_languages_the_project_declares() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let log = plan_log();
        let line = |code: &str| crate::platform::i18n::translate("prompt.languages", Locale::EnUs).replace("{text}", "en-US").replace("{code}", code);
        let mut missing = Vec::new();
        for (config, code) in [(r#"{"language":{"text":"en-US","code":"pt-BR"}}"#, "pt-BR"), (r#"{"language":{"text":"en-US"}}"#, "en-US")] {
            std::fs::write(root.join("mustard.json"), config).unwrap();
            let expected = line(code);
            assert!(expected.contains(&format!("in {code}: names")), "{expected}");
            let built = prompts(root, "teste", &log, Locale::EnUs, &Flight::default(), &log.planned_waves());
            let wave = built.first().expect("the first wave's request").text.clone();
            let last = final_review(root, "teste", &log, Locale::EnUs).text;
            for (name, text) in [("wave", wave), ("final review", last)] {
                let header = text.split("\n\n").take(3).collect::<Vec<_>>();
                if text.matches(&expected).count() != 1 || !header.contains(&expected.as_str()) {
                    missing.push(format!("{config}: the {name} request\n{text}"));
                }
            }
        }
        assert!(missing.is_empty(), "requests without the languages line in the header:\n{}", missing.join("\n\n"));
    }

    /// As linhas de `body` que citam o nome de teste (`test_name`) junto de
    /// um idioma do texto (`text_languages`), com a linha contada de 0.
    fn lines_tying_a_test_name_to_the_text_language<'a>(body: &'a str, test_name: &str, text_languages: [&str; 2]) -> Vec<(usize, &'a str)> {
        body.lines()
            .enumerate()
            .filter(|(_, line)| {
                let lower = line.to_lowercase();
                lower.contains(test_name) && text_languages.iter().any(|language| lower.contains(language))
            })
            .collect()
    }

    /// O cabeçalho de todo pedido põe o nome de teste na lista do código, e a
    /// lista vale mesmo quando o arquivo já traz nomes em outro idioma; a
    /// parte do texto (comentários, entregas e commits) não o cita. E nenhum
    /// molde de agente junta o nome de teste ao idioma do texto: o molde da
    /// onda diz que ele é código e segue o idioma do código. Sem isso o
    /// agente lê "comentário e nome de teste" como texto e escreve o teste
    /// em português num projeto de código em inglês. Nos dois idiomas.
    #[test]
    fn the_header_lists_tests_with_the_code_and_no_template_ties_a_test_name_to_the_text_language() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let log = plan_log();
        let old_pt = "- Comentários seguem o idioma do texto e, como o nome de teste, descrevem o comportamento.";
        let old_en = "- Comments follow the text language and, like the test name, describe behavior.";
        assert_eq!(
            lines_tying_a_test_name_to_the_text_language(old_pt, "nome de teste", ["idioma do texto", "idioma do projeto"]),
            vec![(0, old_pt)],
            "the sentence that ties the test name to the text is not pointed out",
        );
        assert_eq!(
            lines_tying_a_test_name_to_the_text_language(old_en, "test name", ["text language", "project's language"]),
            vec![(0, old_en)],
            "the English sentence that ties the test name to the text is not pointed out",
        );
        for (config, locale, code_marker, tests, other_names, test_name, code_language, text_languages) in [
            (
                r#"{"language":{"text":"pt-BR","code":"en-US"}}"#,
                Locale::PtBr,
                "O código sai em",
                "testes",
                "mesmo quando o arquivo já traz nomes em outro idioma",
                "nome de teste",
                "idioma do código",
                ["idioma do texto", "idioma do projeto"],
            ),
            (
                r#"{"language":{"text":"en-US","code":"pt-BR"}}"#,
                Locale::EnUs,
                "Code is written in",
                "tests",
                "even when the file already has names in another language",
                "test name",
                "code language",
                ["text language", "project's language"],
            ),
        ] {
            std::fs::write(root.join("mustard.json"), config).unwrap();
            let wave = prompts(root, "teste", &log, locale, &Flight::default(), &log.planned_waves()).first().expect("the first wave's request").text.clone();
            let last = final_review(root, "teste", &log, locale).text;
            for (name, request) in [("wave", wave), ("final review", last)] {
                let header = request
                    .split("\n\n")
                    .take(3)
                    .find(|part| part.contains(code_marker))
                    .unwrap_or_else(|| panic!("{locale:?}: the {name} request has no languages line:\n{request}"));
                let (text_part, code_part) = header.split_once(code_marker).expect("the line holds the code marker");
                assert!(!text_part.contains(tests), "{locale:?}: the {name} request puts tests with the text: {header}");
                assert!(code_part.contains(tests), "{locale:?}: the {name} request leaves tests out of the code list: {header}");
                assert!(code_part.contains(other_names), "{locale:?}: the {name} request does not say the list holds over the file's names: {header}");
            }

            let mut tied = Vec::new();
            for (agent, body) in crate::platform::seeds::agent_texts(locale) {
                if agent == "wave" {
                    assert!(
                        body.lines().any(|line| {
                            let lower = line.to_lowercase();
                            lower.contains(test_name) && lower.contains(code_language)
                        }),
                        "{locale:?}: the wave template does not say a test name follows the code language",
                    );
                }
                for (index, line) in lines_tying_a_test_name_to_the_text_language(body, test_name, text_languages) {
                    tied.push(format!("{locale:?} {agent}.md:{}: {line}", index + 1));
                }
            }
            assert!(tied.is_empty(), "templates that tie a test name to the text language:\n{}", tied.join("\n"));
        }
    }

    /// O pedido da onda lista cada item numa linha — o tipo por extenso, o
    /// código e o título —, sem o texto nem a parte do agente: a tarefa é um
    /// passo de "O que fazer", com o que ela atende logo abaixo, e a regra e a
    /// decisão que valem para a onda vão em "O que obedecer". O contexto e o
    /// critério que nenhuma tarefa atende, a onda e o que as ondas anteriores
    /// entregaram não entram. A ordem das tarefas é a que a onda declara, e os
    /// comandos de leitura aparecem uma vez só, na seção de como ler, com o
    /// caminho do repositório principal.
    #[test]
    fn the_wave_request_lists_each_item_in_one_line_and_leaves_out_what_no_task_attends() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let everywhere = json!({"files": ["**"]});
        let log = log_of(&[
            (
                "context",
                json!({"title": "O objetivo", "text": "A obra encurta o pedido. Ele cabe numa leitura.",
                               "agent": "- medir o pedido\n\n  - contar as linhas", "keys": ["objetivo"],
                               "label": "objetivo"}),
            ),
            (
                "context",
                json!({"text": "Como reproduzir. Rode a rodada duas vezes.\n\nConfira o log.",
                               "keys": ["reproduzir"], "label": "reproduzir"}),
            ),
            (
                "decision",
                json!({"title": "Título no lugar do texto", "text": "O agente lê menos. O porquê fica na spec.",
                                "agent": "- `item_line`", "keys": ["títulos"], "why": "menos leitura",
                                "applies_to": everywhere}),
            ),
            (
                "rule",
                json!({"title": "Um exemplo só", "text": "A linha de leitura aparece uma vez.",
                            "agent": "- contar os comandos", "keys": ["exemplo"], "example": "um pedido de exemplo",
                            "applies_to": everywhere}),
            ),
            (
                "criterion",
                json!({"title": "A lista sai curta", "when": "a rodada monta", "then": "cada item tem uma linha",
                                 "proof": "cargo test -p criterio"}),
            ),
            ("criterion", json!({"when": "o critério é antigo", "then": "ele fica sem corpo", "proof": "cargo test -p velho"})),
            (
                "wave",
                json!({"n": 1, "text": "A onda. Ela abre o pedido.", "criteria": [5, 6], "done_when": "passa",
                            "order": [9, 8]}),
            ),
            (
                "task",
                json!({"wave": 1, "title": "Montar a lista", "text": "Hoje a lista só tem códigos.",
                            "agent": "- src/a.rs: `items`\n  - teste: `cargo test -p lista`", "covers": [5, 6],
                            "files": [{"path": "src/a.rs"}]}),
            ),
            (
                "task",
                json!({"wave": 1, "text": "Trocar o texto. Ele diz como ler.", "covers": [1],
                            "files": [{"path": "src/b.rs"}]}),
            ),
            ("delivered", json!({"wave": 1, "text": "A lista saiu.", "files": ["src/a.rs"]})),
        ]);
        let copy = WaveCopy { path: "/c/um".into(), reused: None };
        let flight = Flight { running: [1].into(), copies: [(1, copy)].into(), ..Flight::default() };
        let built = prompts(root, "teste", &log, Locale::PtBr, &flight, &log.planned_waves());
        let part = |key: &str| crate::platform::i18n::translate(key, Locale::PtBr);
        let example = part("prompt.read.wave").replace("{root}", &format!("--root {} ", shown(root))).replace("{spec}", "teste").replace("{wave}", "1");
        assert!(example.contains(&format!("`mustard-rt run read dispatch-1 --root {} --spec teste`", shown(root))), "{example}");
        let wave = &built[0].text;

        // A onda tem `order: [9, 8]`: a tarefa 2 (id 9) vem antes da 1 (id 8),
        // e só a primeira vem inteira, com o que ela atende.
        let to_do: Vec<&str> = wave
            .split_once(&format!("## {}\n\n", part("prompt.part.do")))
            .map(|(_, rest)| rest)
            .unwrap_or_default()
            .split("\n\n")
            .next()
            .unwrap_or_default()
            .lines()
            .collect();
        assert_eq!(
            to_do,
            [
                "1. Leia o texto inteiro de cada item de \"O que obedecer\".".to_string(),
                format!("2. {}", part("prompt.step.tasks")),
                "   - MSTD-TASK-0002 — Trocar o texto. (a primeira, logo abaixo)".to_string(),
                "   - MSTD-TASK-0001 — Montar a lista".to_string(),
                "3. Grave a entrega, como diz \"O que devolver\".".to_string(),
            ],
            "{wave}"
        );
        assert_eq!(
            section_lines(wave, part("prompt.part.obey")),
            ["Decisão MSTD-DEC-0001 — Título no lugar do texto", "Regra MSTD-RULE-0001 — Um exemplo só", "Lições: nenhuma vale para os arquivos desta onda.",],
            "{wave}"
        );

        assert!(wave.contains(&example), "{wave}");
        // Os dois comandos de leitura, só na seção de como ler; o `--root`
        // também vai nos dois de gravar e na linha que manda rodar de dentro
        // da cópia.
        assert_eq!(wave.matches("mustard-rt run read").count(), 2, "{wave}");
        assert_eq!(wave.matches("--root").count(), 5, "{wave}");
        assert!(!wave.contains("--term"), "the batch reads the lessons too: {wave}");
        for (code, times) in [("MSTD-TASK-0002", 2), ("MSTD-TASK-0001", 1), ("MSTD-CTX-0001", 1)] {
            assert_eq!(wave.matches(code).count(), times, "{code}: {wave}");
        }
        assert!(wave.contains("- medir o pedido\n\n  - contar as linhas"), "the first task brings what it attends whole: {wave}");
        assert!(built[0].tasks[1].text.contains("cada item tem uma linha"), "the second task brings its criteria: {}", built[0].tasks[1].text);
        for out in ["MSTD-WAVE-0001", "MSTD-CTX-0002", "MSTD-DELIV-0001", "MSTD-CRIT-0001", "MSTD-CRIT-0002"] {
            assert!(!wave.contains(out), "{out} não é item deste pedido: {wave}");
        }
        for copied in [
            "A obra encurta o pedido",
            "O agente lê menos",
            "menos leitura",
            "A linha de leitura aparece uma vez",
            "um pedido de exemplo",
            "Ela abre o pedido",
            "Como reproduzir",
            "a rodada monta",
            "cada item tem uma linha",
            "cargo test -p criterio",
            "o critério é antigo",
            "cargo test -p velho",
            "Hoje a lista só tem códigos",
            "`items`",
            "A lista saiu.",
        ] {
            assert!(!wave.contains(copied), "{copied} foi copiado: {wave}");
        }
    }

    /// O pedido tem seis seções, sempre na mesma ordem — o que a onda entrega,
    /// como ler cada item, o que fazer, o que obedecer, o que devolver e como
    /// trabalhar — e nenhuma outra: a onda, os critérios, a especificação, o
    /// combinado e o que as ondas anteriores entregaram não têm título
    /// próprio. "Como trabalhar" só aparece quando a onda tem cópia ou o
    /// projeto declara um comando de compilar.
    #[test]
    fn the_wave_request_has_six_sections_in_order_and_no_other() {
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
        let part = |key: &str| crate::platform::i18n::translate(key, Locale::PtBr);
        let five = ["delivers", "read", "do", "obey", "return"].map(|name| format!("## {}", part(&format!("prompt.part.{name}"))));
        let with_copy = Flight { running: [2].into(), copies: [(2, WaveCopy { path: "/c/dois".into(), reused: None })].into(), ..Flight::default() };
        for (flight, sixth) in [(&Flight::default(), false), (&with_copy, true)] {
            let built = prompts(root, "teste", &log, Locale::PtBr, flight, &log.planned_waves());
            let wave = &built.iter().find(|p| p.wave == 2).expect("onda 2 no plano").text;
            let headings: Vec<String> = wave.lines().filter(|line| line.starts_with("## ")).map(String::from).collect();
            let mut expected = five.to_vec();
            if sixth {
                expected.push(format!("## {}", part("prompt.part.work")));
            }
            assert_eq!(headings, expected, "{wave}");
            assert!(!wave.contains("Entregue.") && !wave.contains("MSTD-DELIV"), "{wave}");
        }
    }

    /// As linhas de uma seção do pedido, sem o "- " do começo; vazio quando
    /// a seção não está lá.
    fn section_lines<'a>(text: &'a str, heading: &str) -> Vec<&'a str> {
        let Some((_, after)) = text.split_once(&format!("## {heading}")) else {
            return Vec::new();
        };
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
        let strong: Vec<String> = (1..=5).map(|n| format!("Somar o total da fatura errou o relatório {n}.")).collect();
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
            (
                "task",
                json!({"wave": 1, "text": "Somar o total da fatura no relatório.",
                            "files": [{"path": "apps/rt/src/a.rs"}]}),
            ),
        ]);

        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        // Ids 2 a 6 no banco: os cinco defeitos fortes, na ordem em que
        // entraram — `loose[0]` ocupa o 1, e o sexto e o oitavo (o fraco e o
        // outro solto) ficam fora.
        let expected: Vec<String> = (2u64..=6).zip(&strong).map(|(id, text)| format!("Lição {id} — {text}")).collect();
        let obey = section_lines(&built[0].text, crate::platform::i18n::translate("prompt.part.obey", Locale::PtBr));
        let lessons: Vec<&str> = obey.iter().copied().filter(|line| line.starts_with("Lição ")).collect();
        assert_eq!(lessons, expected.iter().map(String::as_str).collect::<Vec<_>>(), "o pedido da onda: {}", built[0].text);
        for out in [weak, loose[0], loose[1], "Resposta curta."] {
            assert!(!built[0].text.contains(out), "{out} não é lição desta onda: {}", built[0].text);
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
            keyed_line(
                1,
                "defect",
                DISPATCH_LESSON,
                &["pedido", "onda", "subagente", "despacho", "contexto"],
                everywhere.clone(),
                json!({"spec": "mustard-enxuto"}),
            ),
            keyed_line(
                3,
                "defect",
                "Subagente nunca roda compilação ou teste em segundo plano: o aviso de término não chega a ele, e ele encerra dizendo que espera a suíte, com o trabalho pela metade. Rode tudo em primeiro plano, com o teto de tempo do comando, e só devolva o relatório depois de ver o resultado. Em 17/09 duas revisões pararam assim e precisaram ser retomadas.",
                &["segundo plano", "suíte", "subagente", "revisão", "espera"],
                everywhere.clone(),
                json!({"spec": "mustard-enxuto"}),
            ),
            keyed_line(
                4,
                "defect",
                "Quem tira uma proteção (trava, reserva, recusa) prova que o que ela protegia continua protegido: a trava que sobra cobre o bloco inteiro (ler, juntar, gravar, comitar e desfazer), e o teste roda duas voltas ao mesmo tempo, não só uma de cada vez. Em 17/09 a onda 33 tirou a reserva de arquivos e a trava do git seguiu cobrindo só o commit: duas rodadas simultâneas no mesmo arquivo perdiam a mudança de uma delas para sempre.",
                &["trava", "reserva", "ao mesmo tempo", "concorrência", "proteção"],
                everywhere.clone(),
                json!({"spec": "mustard-enxuto"}),
            ),
            keyed_line(
                96,
                "defect",
                "O teste tem de falhar quando o código está errado. Teste na divisa: o último valor que passa e o primeiro que já não passa. Teste pelo caminho que a pessoa usa, com os ganchos e os comandos, sem montar a conversa à mão. E teste com os dados completos de uma sessão real. Em 18/09, três testes desta obra passavam com o código errado por falta disso.",
                &["teste", "prova", "vermelho", "divisa", "caminho real", "dados completos"],
                everywhere,
                json!({"spec": "conferencia-instalacao-barra"}),
            ),
        ];
        let path = root.join(".claude").join("spec");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("lessons.ndjson"), lines.concat()).unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 7, "text": "Os dois ajustes pedidos depois das revisões", "criteria": [], "done_when": "passa"})),
            (
                "task",
                json!({"wave": 7, "label": "Onda 7, a barra sem a contagem de linhas mudadas", "text": STATUS_BAR_TASK,
                            "files": [{"path": "apps/rt/src/commands/statusline/mod.rs"}, {"path": "apps/rt/src/commands/statusline/segment.rs"}]}),
            ),
        ]);

        let bank = crate::io::lessons::read(&path.join("lessons.ndjson")).unwrap().unwrap();
        let by_text = crate::domain::lessons::matching_among(&crate::domain::lessons::kept(&bank), STATUS_BAR_TASK, &Languages::new(["pt-BR", "en-US"]));
        assert!(by_text.iter().any(|hit| hit.id == 1), "pelo texto inteiro, a lição do envio entraria: {by_text:?}");

        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let part = |key: &str| crate::platform::i18n::translate(key, Locale::PtBr);
        let obey = section_lines(&built[0].text, part("prompt.part.obey"));
        let lessons: Vec<&str> = obey.iter().copied().filter(|line| line.starts_with("Lição ")).collect();
        assert_eq!(lessons, ["Lição 96 — O teste tem de falhar quando o código está errado."], "{lessons:?}");
        for out in [DISPATCH_LESSON, "Subagente nunca roda", "Quem tira uma proteção", "Teste na divisa"] {
            assert!(!built[0].text.contains(out), "{out} não deve aparecer: só o título da lição entra");
        }
    }

    /// Os envios reais das treze ondas de uma obra deste projeto, com as
    /// lições do banco que chegaram a cada uma, as que o orquestrador tirou
    /// à mão (53) e as que ele manteve. Cada onda é remontada com os arquivos
    /// e o texto reais das tarefas dela e um banco só com as lições que
    /// chegaram a ela, e passa pela mesma leitura da rodada e do pedido. As
    /// lições que servem à onda evitam 18 das tiradas — 11 nas duas ondas só
    /// de markdown, 7 pelo arquivo citado que a onda não toca — e nenhuma das
    /// mantidas se perde.
    #[test]
    fn the_lessons_the_orchestrator_removed_do_not_arrive_and_the_kept_ones_continue() {
        let fixture: Value = serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/lessons-by-wave.json"))).unwrap();
        let ids = |value: &Value| -> BTreeSet<u64> { value.as_array().unwrap().iter().map(|id| id.as_u64().unwrap()).collect() };
        let lessons: BTreeMap<u64, &Value> = fixture["lessons"].as_array().unwrap().iter().map(|l| (l["id"].as_u64().unwrap(), l)).collect();
        let expected: BTreeMap<u64, BTreeSet<u64>> = [
            (2, vec![52]),
            (3, vec![98]),
            (4, vec![52]),
            (6, vec![3, 11, 12, 13, 14, 80, 101]),
            (7, vec![52]),
            (8, vec![52, 95]),
            (9, vec![96, 101, 102, 110]),
            (10, vec![98]),
        ]
        .into_iter()
        .map(|(n, avoided)| (n, avoided.into_iter().collect()))
        .collect();

        let mut removed_total = 0;
        let mut avoided_total = 0;
        for wave in fixture["waves"].as_array().unwrap() {
            let n = wave["n"].as_u64().unwrap();
            let (arrived, removed, kept) = (ids(&wave["arrived"]), ids(&wave["removed"]), ids(&wave["kept"]));
            assert_eq!(arrived, removed.union(&kept).copied().collect(), "onda {n}: chegaram as tiradas e as mantidas");
            removed_total += removed.len();

            let bank_text: String = arrived
                .iter()
                .map(|id| {
                    let l = lessons[id];
                    let keys: Vec<&str> = l["keys"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
                    keyed_line(*id, l["class"].as_str().unwrap(), l["text"].as_str().unwrap(), &keys, l["applies_to"].clone(), json!({"spec": "obra-real"}))
                })
                .collect();
            let bank = crate::domain::spec_events::parse_log(&bank_text);
            let paths: Vec<Value> = wave["files"].as_array().unwrap().iter().map(|path| json!({"path": path})).collect();
            let log = log_of(&[
                ("wave", json!({"n": n, "text": "A onda", "criteria": [], "done_when": "passa"})),
                ("task", json!({"wave": n, "text": wave["text"], "files": paths})),
            ]);

            let got: BTreeSet<u64> = wave_lessons(&bank, &log, n, &Languages::new(["pt-BR", "en-US"])).iter().map(|l| l.id).collect();
            let avoided: BTreeSet<u64> = arrived.difference(&got).copied().collect();
            assert!(got.is_subset(&arrived), "onda {n}: {got:?} fora de {arrived:?}");
            let lost: Vec<&u64> = kept.difference(&got).collect();
            assert!(lost.is_empty(), "onda {n}: perdeu as mantidas {lost:?}");
            assert_eq!(avoided, expected.get(&n).cloned().unwrap_or_default(), "onda {n}");
            avoided_total += avoided.len();
        }
        assert_eq!(removed_total, 53);
        assert_eq!(avoided_total, 18);
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
        let strong: Vec<String> = (1..=5).map(|n| format!("Somar o total da fatura exige conferir o relatório {n}.")).collect();
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
            (
                "task",
                json!({"wave": 1, "text": "Somar o total da fatura no relatório.",
                            "files": [{"path": "apps/rt/src/a.rs"}]}),
            ),
        ]);

        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let obey = section_lines(&built[0].text, crate::platform::i18n::translate("prompt.part.obey", Locale::PtBr));
        let shown: Vec<&str> = obey.iter().copied().filter(|line| line.starts_with("Lição ")).collect();
        // Ids 501 a 505: as cinco regras fortes, gravadas depois das 500
        // "Regra N"; id 509: a preferência, cujo texto também divide
        // "total" e "fatura" com a tarefa. Id 508, o defeito sem palavra em
        // comum, fica fora.
        let mut expected: Vec<String> = (501u64..=505).zip(&strong).map(|(id, text)| format!("Lição {id} — {text}")).collect();
        expected.push("Lição 509 — O total da fatura sai em reais.".to_string());
        assert_eq!(shown, expected.iter().map(String::as_str).collect::<Vec<_>>(), "{shown:?}");
        for out in weak.iter().map(String::as_str).chain(["Apagar a pasta perde trabalho."]) {
            assert!(!built[0].text.contains(out), "{out} não é lição desta onda");
        }
        assert_eq!(shown.len(), 6, "{shown:?}");
    }

    /// Uma skill que cita dois arquivos — o segundo escrito como `cited` —,
    /// com a data do próprio arquivo em `written` (segundos desde 1970), num
    /// projeto cujo mapa guarda um commit em `moved` que mudou o segundo
    /// citado, e com o mapa só quando `with_map`; devolve o pedido da onda.
    /// Nenhum evento `skill` é gravado: a marca sai só das datas.
    fn request_with_skill_written_at(written: i64, moved: i64, with_map: bool, cited: &str) -> WavePrompt {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("apps/rt/src")).unwrap();
        std::fs::write(root.join("apps/rt/src/a.rs"), "fn um() {}\n").unwrap();
        std::fs::write(root.join("apps/rt/src/exemplo.rs"), "fn dois() {}\n").unwrap();
        write_skill(root, "apps/rt", "somar", &format!("# O molde\n\nExemplos usados: `apps/rt/src/a.rs` e `{cited}`.\n"));
        let skill = root.join("apps/rt/.claude/skills/somar/SKILL.md");
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(u64::try_from(written).unwrap());
        std::fs::File::options().write(true).open(&skill).unwrap().set_modified(at).unwrap();
        if with_map {
            write_map(
                root,
                &json!({
                    "modules": [{"path": "apps/rt/src/a.rs"}, {"path": "apps/rt/src/exemplo.rs"}],
                    "history": {
                        "paths": ["apps/rt/src/a.rs", "apps/rt/src/exemplo.rs"],
                        "commits": [
                            {"id": "abc1234", "at": written - 7_200, "changed": [0]},
                            {"id": "def5678", "at": moved, "changed": [1]},
                        ],
                    }
                }),
            );
        }
        prompts(root, "teste", &plan_log(), Locale::PtBr, &Flight::default(), &plan_log().planned_waves()).remove(0)
    }

    /// Um projeto em que o pedido da onda usa cada parte do mapa: a leitura
    /// obrigatória e a skill citam caminhos só pelo fim, que só o mapa acha;
    /// a função citada tem as linhas no mapa, num arquivo cujo fim também
    /// casa com outro, que vem antes e não a declara; o arquivo da tarefa tem
    /// teste conhecido; e o arquivo que a skill cita mudou depois dela. Com
    /// `broken`, cada arquivo guarda um texto onde o mapa guarda a lista das
    /// importações, coluna que o pedido não lê. Devolve a pasta e o plano.
    fn project_using_every_map_part(broken: bool) -> (tempfile::TempDir, SpecLog) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let written: i64 = 1_789_000_000;
        write_skill(root, "apps/rt", "somar", "# O molde\n\nExemplo usado: `src/exemplo.rs`.\n");
        let skill = root.join("apps/rt/.claude/skills/somar/SKILL.md");
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(u64::try_from(written).unwrap());
        std::fs::File::options().write(true).open(&skill).unwrap().set_modified(at).unwrap();
        let deps = if broken { json!("um texto no lugar da lista") } else { json!(["apps/rt/src/exemplo.rs"]) };
        write_map(
            root,
            &json!({
                "modules": [
                    {"path": "lib/src/a.rs", "declarations": [{"kind": "function", "name": "outra", "line": 1, "end_line": 2}]},
                    {"path": "apps/rt/src/a.rs", "tests": ["apps/rt/tests/a_test.rs"], "deps": deps,
                     "declarations": [{"kind": "function", "name": "soma", "line": 3, "end_line": 5},
                                      {"kind": "function", "name": "soma", "line": 9, "end_line": 12}]},
                    {"path": "apps/rt/src/exemplo.rs", "deps": deps},
                    {"path": "apps/rt/tests/a_test.rs", "deps": ["apps/rt/src/a.rs"]}
                ],
                "history": {
                    "paths": ["apps/rt/src/a.rs", "apps/rt/src/exemplo.rs"],
                    "commits": [{"id": "abc1234", "at": written - 7_200, "changed": [0]}, {"id": "def5678", "at": written + 3_600, "changed": [1]}]
                }
            }),
        );
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            (
                "task",
                json!({"wave": 1, "text": "Somar", "skill": "somar", "files": [{"path": "apps/rt/src/a.rs"}],
                            "must_read": ["rt/src/a.rs#soma", "src/a.rs#soma", "lib/src/a.rs#outra"]}),
            ),
        ]);
        (dir, log)
    }

    /// O pedido da onda lê cada parte do mapa pela pergunta dela e sai igual,
    /// byte a byte, ao que saía com o mapa inteiro lido.
    #[test]
    fn the_wave_request_reading_only_its_map_parts_is_the_one_the_whole_map_gave() {
        let (dir, log) = project_using_every_map_part(false);
        let root = dir.path();
        let by_question = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let whole = prompts_reading(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves(), &|_| crate::io::project_map::read(root), &|_, _| false);
        assert_eq!(by_question, whole);
        let text = &by_question[0].text;
        assert!(text.contains("leia só as linhas 3-5, 9-12 de `soma` em `rt/src/a.rs`"), "{text}");
        assert!(text.contains("leia só `soma` em `src/a.rs`"), "the first file ending in the path wins: {text}");
        assert!(text.contains("Quem testa `apps/rt/src/a.rs`: `apps/rt/tests/a_test.rs`"), "{text}");
        assert_eq!(by_question[0].stale_skills, ["somar"]);
    }

    /// Um mapa em que uma coluna que o pedido não lê guarda o tipo errado: a
    /// leitura do mapa inteiro o recusa, e o pedido, que lê só os caminhos,
    /// as declarações com o nome, os testes do arquivo e a história, ainda
    /// traz as linhas, os testes, a skill e a marca de revisar.
    #[test]
    fn the_wave_request_reads_its_map_parts_even_when_a_column_it_does_not_read_is_broken() {
        let (dir, log) = project_using_every_map_part(true);
        let root = dir.path();
        assert!(crate::io::project_map::read(root).is_err(), "the whole map refuses the broken column");
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let text = &built[0].text;
        assert!(built[0].bad_skills.is_empty(), "the map finds the path the skill cites: {:?}", built[0].bad_skills);
        assert!(text.contains("leia só as linhas 3-5, 9-12 de `soma` em `rt/src/a.rs`"), "{text}");
        assert!(text.contains("Quem testa `apps/rt/src/a.rs`: `apps/rt/tests/a_test.rs`"), "{text}");
        assert_eq!(built[0].stale_skills, ["somar"]);
    }

    /// Um projeto com dois pares de papéis, cada um com regra forte:
    /// controller importa service em 24 de 25 importações (a que vai contra
    /// é do `service4`, testado e o mais recente da pasta) e repository
    /// importa entity em todas as 25. Cada arquivo declara a classe com o
    /// nome dele, com as linhas, e o código dele está no disco. A onda tem
    /// uma tarefa que cria um controller e outra que cria um service.
    fn project_with_a_pattern() -> (tempfile::TempDir, SpecLog) {
        project_with_a_pattern_read_before(&[])
    }

    /// [`project_with_a_pattern`] com o mapa lido antes de os arquivos
    /// `stale` mudarem: o conteúdo que ele guarda deles não é o do disco.
    fn project_with_a_pattern_read_before(stale: &[&str]) -> (tempfile::TempDir, SpecLog) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let named = |role: &str, n: usize| format!("src/{role}/{role}{n}.{role}.ts");
        let mut modules = Vec::new();
        for (from, to) in [("controller", "service"), ("repository", "entity")] {
            for a in 0..5 {
                let mut deps: Vec<String> = (0..5).map(|b| named(to, b)).collect();
                if from == "controller" && a == 4 {
                    deps.pop();
                }
                modules.push(json!({"path": named(from, a), "language": "typescript", "loc": 50, "deps": deps,
                    "declarations": [{"kind": "class", "name": format!("{}{a}", capitalized(from)), "line": 3, "end_line": 9}]}));
            }
            for b in 0..5 {
                let against = from == "controller" && b == 4;
                let deps = if against { vec![named(from, 4)] } else { Vec::new() };
                let tests = if against { vec![format!("test/{to}{b}.spec.ts")] } else { Vec::new() };
                modules.push(json!({"path": named(to, b), "language": "typescript", "loc": 50, "deps": deps, "tests": tests,
                    "declarations": [{"kind": "class", "name": format!("{}{b}", capitalized(to)), "line": 2, "end_line": 8}]}));
            }
        }
        for module in &mut modules {
            let path = root.join(module["path"].as_str().unwrap());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "export class Example {\n  handle() {\n    return this.orders.list();\n  }\n}\n").unwrap();
            if stale.contains(&module["path"].as_str().unwrap()) {
                module["blob"] = json!("1111111111111111111111111111111111111111");
            }
        }
        write_map(
            root,
            &json!({
                "modules": modules,
                "history": {"paths": [named("service", 4)], "commits": [{"id": "abc1234", "at": 1_789_000_000, "changed": [0]}]}
            }),
        );
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Criar o controller", "files": [{"path": named("controller", 5)}]})),
            ("task", json!({"wave": 1, "text": "Criar o service", "files": [{"path": named("service", 5)}]})),
        ]);
        (dir, log)
    }

    fn capitalized(word: &str) -> String {
        let mut chars = word.chars();
        chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
    }

    /// As linhas do bloco de uma tarefa, pelo título dela: o pedido traz o da
    /// primeira, e o envio guarda o de todas.
    fn task_lines<'t>(prompt: &'t WavePrompt, title: &str) -> Vec<&'t str> {
        let block = prompt.tasks.iter().find(|block| block.text.lines().next().is_some_and(|line| line.contains(title)));
        block.map(|block| block.text.lines().collect()).unwrap_or_default()
    }

    #[test]
    fn a_task_request_carries_the_rules_of_its_roles_and_examples_that_follow_them() {
        let (dir, log) = project_with_a_pattern();
        let root = dir.path();
        let by_question = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let whole = prompts_reading(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves(), &|_| crate::io::project_map::read(root), &|_, _| false);
        assert_eq!(by_question, whole, "the map parts give the same pattern as the whole map");
        let text = &by_question[0].text;
        let controller = task_lines(&by_question[0], "Criar o controller").join("\n");
        assert!(controller.contains("regra: controller importa service em 24 de 25 importações"), "{controller}");
        // Só a regra do papel que a tarefa toca: a do repository fica fora.
        assert!(!controller.contains("repository"), "{controller}");
        assert!(controller.contains("exemplo: `Controller0` em `src/controller/controller0.controller.ts`, linhas 3 a 9"), "{controller}");
        let service = task_lines(&by_question[0], "Criar o service").join("\n");
        assert!(service.contains("regra: controller importa service"), "{service}");
        assert!(service.contains("exemplo: `Service0`"), "{service}");
        // O service que importa um controller vai contra a regra: nunca é
        // exemplo, mesmo testado e o mais recente da pasta.
        assert!(!service.contains("service4"), "{service}");
        // O exemplo é nome, arquivo e linhas: o código dele não entra.
        assert!(!text.contains("return this.orders"), "{text}");
        assert!(!text.contains("export class"), "{text}");
    }

    /// O exemplo cujo arquivo mudou depois da passada que gravou o mapa — o
    /// commit que a passada em segundo plano ainda não leu — sai pelo nome e
    /// pelo arquivo, sem as linhas velhas do mapa.
    #[test]
    fn an_example_whose_file_changed_after_the_map_goes_by_name_without_lines() {
        let (dir, log) = project_with_a_pattern_read_before(&["src/controller/controller0.controller.ts"]);
        let root = dir.path();
        let built = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let controller = task_lines(&built[0], "Criar o controller").join("\n");
        assert!(controller.contains("exemplo: `Controller0` em `src/controller/controller0.controller.ts`"), "{controller}");
        assert!(!controller.contains("controller0.controller.ts`, linhas"), "{controller}");
        let service = task_lines(&built[0], "Criar o service").join("\n");
        assert!(service.contains("exemplo: `Service0` em `src/service/service0.service.ts`, linhas 2 a 8"), "{service}");
    }

    #[test]
    fn a_project_without_rules_gets_no_pattern_block() {
        let (dir, log) = project_using_every_map_part(false);
        let built = prompts(dir.path(), "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let head = crate::platform::i18n::translate("prompt.pattern.head", Locale::PtBr);
        assert!(!built[0].text.contains(head), "{}", built[0].text);
        assert!(!built[0].text.contains("regra:") && !built[0].text.contains("costume:"), "{}", built[0].text);
    }

    /// Um arquivo com o texto de sempre, na pasta do projeto `root`.
    fn touch(root: &Path, path: &str) {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, "pub fn run() {}\n").unwrap();
    }

    /// Um projeto sem regra de importação: vinte e cinco arquivos medidos,
    /// entre eles o maior do projeto, `src/big.rs`, e uma tarefa em cada um
    /// dos dois, o grande e um pequeno.
    fn project_with_one_large_file() -> (tempfile::TempDir, SpecLog) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let mut modules = vec![json!({"path": "src/big.rs", "language": "rust", "loc": 5200, "quality": {"size": 5000}})];
        for n in 0..24 {
            modules.push(json!({"path": format!("src/small{n}.rs"), "language": "rust", "loc": 100 + n, "quality": {"size": 90 + n}}));
        }
        for module in &modules {
            touch(root, module["path"].as_str().unwrap());
        }
        write_map(root, &json!({"modules": modules}));
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Mexer no grande", "files": [{"path": "src/big.rs"}]})),
            ("task", json!({"wave": 1, "text": "Mexer no pequeno", "files": [{"path": "src/small3.rs"}]})),
        ]);
        (dir, log)
    }

    #[test]
    fn a_task_touching_the_largest_file_gets_the_line_and_one_on_a_small_file_does_not() {
        let (dir, log) = project_with_one_large_file();
        let built = prompts(dir.path(), "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        assert_eq!(built.len(), 1);
        let text = &built[0].text;
        let big = task_lines(&built[0], "Mexer no grande").join("\n");
        assert!(big.contains("Entre os 5% maiores arquivos do projeto: `src/big.rs`. Ponha o código novo num arquivo novo."), "{big}");
        let head = crate::platform::i18n::translate("prompt.pattern.head_plain", Locale::PtBr);
        assert!(big.contains(head), "{big}");
        let small = task_lines(&built[0], "Mexer no pequeno").join("\n");
        assert!(!small.contains("maiores arquivos") && !small.contains(head), "{small}");
        // O tamanho só informa: o pedido sai inteiro, sem recusa.
        assert!(built[0].bad_skills.is_empty() && built[0].stale_skills.is_empty());
        assert!(text.contains("Mexer no pequeno"), "{text}");
    }

    /// Um projeto sem regra de importação cuja história tem dez commits que
    /// criaram um comando em `apps/rt/src/commands`: nove registram o comando
    /// no índice e, com `tested`, sete criam o teste dele. A tarefa cria um
    /// comando novo.
    fn project_creating_a_command(tested: bool) -> (tempfile::TempDir, SpecLog) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let dir_of = "apps/rt/src/commands";
        let mut paths: Vec<String> = vec![format!("{dir_of}/mod.rs"), "packages/core/src/i18n.rs".to_string()];
        for n in 0..10 {
            paths.push(format!("{dir_of}/cmd_{n}.rs"));
            paths.push(format!("apps/rt/tests/cmd_{n}.rs"));
        }
        paths.sort();
        let at = |path: &str| paths.iter().position(|p| p == path).unwrap();
        let commits: Vec<Value> = (0..10)
            .map(|n| {
                let mut added = vec![at(&format!("{dir_of}/cmd_{n}.rs"))];
                if tested && n < 7 {
                    added.push(at(&format!("apps/rt/tests/cmd_{n}.rs")));
                }
                let mut changed = Vec::new();
                if n < 9 {
                    changed.push(at(&format!("{dir_of}/mod.rs")));
                }
                if n < 3 {
                    changed.push(at("packages/core/src/i18n.rs"));
                }
                added.sort_unstable();
                changed.sort_unstable();
                json!({"id": format!("c{n}"), "at": 1_789_000_000 + n, "added": added, "changed": changed})
            })
            .collect();
        let modules: Vec<Value> = paths.iter().filter(|p| !p.contains("/tests/")).map(|p| json!({"path": p, "language": "rust", "loc": 40})).collect();
        for module in &modules {
            touch(root, module["path"].as_str().unwrap());
        }
        write_map(root, &json!({"modules": modules, "history": {"base": "main", "paths": paths, "commits": commits}}));
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Criar o comando", "files": [{"path": format!("{dir_of}/novo.rs")}]})),
        ]);
        (dir, log)
    }

    #[test]
    fn a_task_creating_a_command_gets_the_registry_and_the_test_in_a_block_with_only_the_recipe() {
        let (dir, log) = project_creating_a_command(true);
        let built = prompts(dir.path(), "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let lines = task_lines(&built[0], "Criar o comando").join("\n");
        let head = crate::platform::i18n::translate("prompt.pattern.head_plain", Locale::PtBr);
        assert!(lines.contains(head), "{lines}");
        assert!(lines.contains("Receita do git, de 10 commits que criaram um arquivo `apps/rt/src/commands/*.rs`:"), "{lines}");
        assert!(lines.contains("\n    - mudou `apps/rt/src/commands/mod.rs` em 9 de 10"), "{lines}");
        assert!(lines.contains("\n    - criou um teste em 7 de 10"), "{lines}");
        // O texto mudou junto em só 3 dos 10: fica fora.
        assert!(!lines.contains("i18n.rs"), "{lines}");
        // Sem regra de importação, o bloco não fala de regra nem de exemplo.
        assert!(!lines.contains("regra:") && !lines.contains("exemplo:"), "{lines}");
        // A receita sai do programa: nenhum pedido chama o agente de skill.
        assert_eq!(built[0].agent, "wave");
        assert!(!built[0].text.contains("mustard-skill") && !built[0].text.contains("new_skill"), "{}", built[0].text);
    }

    #[test]
    fn a_project_without_any_test_gets_no_test_line_in_the_recipe() {
        let (dir, log) = project_creating_a_command(false);
        let built = prompts(dir.path(), "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let lines = task_lines(&built[0], "Criar o comando").join("\n");
        assert!(lines.contains("mudou `apps/rt/src/commands/mod.rs` em 9 de 10"), "{lines}");
        assert!(!lines.contains("criou um teste"), "{lines}");
    }

    /// O índice dos comandos, que mudou junto em 9 dos 10 commits, não existe
    /// mais no projeto: sai da receita, e fica a linha do teste. Sem teste
    /// nenhum na história, a receita fica sem nada a dizer e o bloco não sai.
    #[test]
    fn a_file_that_no_longer_exists_leaves_the_recipe_and_a_recipe_left_with_nothing_leaves_the_request() {
        let head = crate::platform::i18n::translate("prompt.pattern.head_plain", Locale::PtBr);
        let (dir, log) = project_creating_a_command(true);
        std::fs::remove_file(dir.path().join("apps/rt/src/commands/mod.rs")).unwrap();
        let built = prompts(dir.path(), "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let lines = task_lines(&built[0], "Criar o comando").join("\n");
        assert!(lines.contains("Receita do git, de 10 commits que criaram um arquivo `apps/rt/src/commands/*.rs`:"), "{lines}");
        assert!(lines.contains("\n    - criou um teste em 7 de 10"), "{lines}");
        assert!(!lines.contains("mod.rs"), "{lines}");

        let (dir, log) = project_creating_a_command(false);
        std::fs::remove_file(dir.path().join("apps/rt/src/commands/mod.rs")).unwrap();
        let built = prompts(dir.path(), "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let lines = task_lines(&built[0], "Criar o comando").join("\n");
        assert!(!lines.contains("Receita do git") && !lines.contains(head), "no block at all: {lines}");
    }

    #[test]
    fn a_new_project_without_history_gets_no_recipe() {
        let (dir, log) = project_creating_a_command(true);
        let mut map = crate::io::project_map::read(dir.path()).unwrap();
        map.history = History::default();
        write_map(dir.path(), &serde_json::to_value(&map).unwrap());
        let built = prompts(dir.path(), "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let lines = task_lines(&built[0], "Criar o comando").join("\n");
        assert!(!lines.contains("Receita do git"), "{lines}");
        let head = crate::platform::i18n::translate("prompt.pattern.head_plain", Locale::PtBr);
        assert!(!lines.contains(head), "no block at all: {lines}");
    }

    /// Um projeto cuja janela do mapa está cheia, com `MAX_COMMITS` commits
    /// que nunca tocam `src/old.rs`: o último commit dele ficou fora dela.
    fn project_with_a_file_older_than_the_window() -> (tempfile::TempDir, SpecLog) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for path in ["src/old.rs", "src/registry.rs", "src/busy.rs"] {
            touch(root, path);
        }
        let commits: Vec<Value> = (0..MAX_COMMITS).map(|n| json!({"id": format!("n{n:05}"), "at": 1_789_000_000 + n, "changed": [0]})).collect();
        let modules = json!([
            {"path": "src/old.rs", "language": "rust", "loc": 30},
            {"path": "src/registry.rs", "language": "rust", "loc": 30},
            {"path": "src/busy.rs", "language": "rust", "loc": 30}
        ]);
        write_map(root, &json!({"modules": modules, "history": {"base": "main", "paths": ["src/busy.rs"], "commits": commits}}));
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Mudar o antigo", "files": [{"path": "src/old.rs"}]})),
        ]);
        (dir, log)
    }

    #[test]
    fn a_file_older_than_the_window_gets_its_history_on_the_spot_and_the_next_request_does_not_read_git_again() {
        let (dir, log) = project_with_a_file_older_than_the_window();
        let root = dir.path();
        let calls = std::cell::Cell::new(0);
        // O `scan history` de mentira: grava no mapa a história do arquivo,
        // como o de verdade, com três commits que mudaram o registro junto.
        let trace = |file: &str, moves: usize| {
            calls.set(calls.get() + 1);
            let mark = crate::io::project_map::read_for(root, Need::Lineage(file)).unwrap().census_mark;
            let commit = |id: &str| crate::domain::project_map::LineageCommit {
                id: id.to_string(),
                files: crate::domain::project_map::CommitFiles { changed: vec![file.to_string(), "src/registry.rs".to_string()], ..Default::default() },
                ..Default::default()
            };
            let lineage = FileLineage {
                path: file.to_string(),
                base: "main".to_string(),
                mark,
                moves: u32::try_from(moves).unwrap(),
                commits: vec![commit("a1"), commit("a2"), commit("a3")],
                ..FileLineage::default()
            };
            crate::io::project_map::save_lineage_at(&crate::io::project_map::model_path(root), &lineage).unwrap();
            true
        };
        let read = |need: Need<'_>| crate::io::project_map::read_for(root, need);
        for round in 0..2 {
            let built = prompts_reading(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves(), &read, &trace);
            let lines = task_lines(&built[0], "Mudar o antigo").join("\n");
            assert!(lines.contains("Receita do git, de 3 commits que mudaram `src/old.rs`:"), "round {round}: {lines}");
            assert!(lines.contains("mudou `src/registry.rs` em 3 de 3"), "round {round}: {lines}");
        }
        assert_eq!(calls.get(), 1, "the second request reads the history kept in the map, not git");
    }

    /// Com a janela do mapa longe de cheia, a história que ela guarda é a
    /// inteira: o arquivo sem commit nela não tem história a ler do git.
    #[test]
    fn a_file_without_commits_in_a_window_that_is_not_full_never_reads_git() {
        let (dir, log) = project_with_one_large_file();
        let root = dir.path();
        let calls = std::cell::Cell::new(0);
        let trace = |_: &str, _: usize| {
            calls.set(calls.get() + 1);
            false
        };
        let read = |need: Need<'_>| crate::io::project_map::read_for(root, need);
        prompts_reading(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves(), &read, &trace);
        assert_eq!(calls.get(), 0);
    }

    /// Os pedidos do projeto da janela cheia
    /// ([`project_with_a_file_older_than_the_window`]) com o `mustard.json`
    /// `config`, ou sem arquivo, quando `None`. Devolve o número de mudanças
    /// de arquivo que a leitura da história pediu ao scan e os pedidos.
    fn moves_asked_with(config: Option<&str>) -> (Vec<usize>, Vec<WavePrompt>) {
        let (dir, log) = project_with_a_file_older_than_the_window();
        let root = dir.path();
        if let Some(config) = config {
            std::fs::write(root.join("mustard.json"), config).unwrap();
        }
        let asked = RefCell::new(Vec::new());
        let trace = |_: &str, moves: usize| {
            asked.borrow_mut().push(moves);
            false
        };
        let read = |need: Need<'_>| crate::io::project_map::read_for(root, need);
        let built = prompts_reading(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves(), &read, &trace);
        (asked.into_inner(), built)
    }

    /// Sem a chave, a história segue o padrão; com ela, o número escrito. Nos
    /// dois casos o pedido não traz aviso nenhum.
    #[test]
    fn the_history_moves_follow_the_config_and_the_default_without_a_warning() {
        let (asked, built) = moves_asked_with(None);
        assert_eq!(asked, [MOVES_FOLLOWED], "no key: the default");
        assert!(built.iter().all(|p| p.bad_settings.is_empty()), "{built:?}");

        let (asked, built) = moves_asked_with(Some(r#"{"map": {"historyMoves": 2}}"#));
        assert_eq!(asked, [2], "the key: its number");
        assert!(built.iter().all(|p| p.bad_settings.is_empty()), "{built:?}");
    }

    /// O número inválido — zero, negativo, fração ou texto — cai no padrão, e
    /// o pedido leva o aviso com a chave e o padrão que valeu, nos dois
    /// idiomas: a montagem não tem sessão, então quem despacha o pedido diz
    /// onde o aviso sai.
    #[test]
    fn an_invalid_history_moves_falls_back_to_the_default_and_the_request_carries_the_warning() {
        for bad in ["0", "-3", "1.5", r#""tres""#] {
            let config = format!(r#"{{"map": {{"historyMoves": {bad}}}}}"#);
            let (asked, built) = moves_asked_with(Some(&config));
            assert_eq!(asked, [MOVES_FOLLOWED], "{bad}: the default is what the history follows");
            assert_eq!(built.len(), 1, "{bad}");
            let [warning] = built[0].bad_settings.as_slice() else { panic!("{bad}: {:?}", built[0].bad_settings) };
            assert_eq!(warning.key, "historyMoves", "{bad}");
            assert!(warning.message.contains("map.historyMoves") && warning.message.contains(&MOVES_FOLLOWED.to_string()), "{bad}: {}", warning.message);
        }
    }

    /// Só avisa quando o padrão valeu de fato: o número inválido que o pedido
    /// nunca leu — nenhuma receita precisou da história além da janela — não
    /// gera aviso.
    #[test]
    fn an_invalid_history_moves_is_not_warned_when_no_recipe_reads_it() {
        let (dir, log) = project_with_one_large_file();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"map": {"historyMoves": 0}}"#).unwrap();
        let read = |need: Need<'_>| crate::io::project_map::read_for(root, need);
        let trace = |_: &str, _: usize| panic!("no recipe reads the history beyond the window");
        let built = prompts_reading(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves(), &read, &trace);
        assert!(built.iter().all(|p| p.bad_settings.is_empty()), "{built:?}");
    }

    /// A receita guardada serve às ondas seguintes sem ler o número de novo:
    /// o aviso vai em todo pedido da montagem, e não só no da onda que o leu
    /// primeiro, porque quem despacha pega só os pedidos das ondas que saem.
    #[test]
    fn the_warning_of_an_invalid_history_moves_goes_in_every_request_of_the_assembly() {
        let (dir, _) = project_with_a_file_older_than_the_window();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"map": {"historyMoves": -1}}"#).unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Mudar o antigo", "files": [{"path": "src/old.rs"}]})),
            ("wave", json!({"n": 2, "text": "Outra onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 2, "text": "Mudar o movimentado", "files": [{"path": "src/busy.rs"}]})),
        ]);
        let read = |need: Need<'_>| crate::io::project_map::read_for(root, need);
        let trace = |_: &str, _: usize| false;
        let built = prompts_reading(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves(), &read, &trace);
        assert_eq!(built.iter().map(|p| p.wave).collect::<Vec<_>>(), [1, 2]);
        for prompt in &built {
            assert_eq!(prompt.bad_settings.len(), 1, "wave {}: {:?}", prompt.wave, prompt.bad_settings);
            assert_eq!(prompt.bad_settings[0].key, "historyMoves");
        }
    }

    /// O pedido marca a skill como a revisar pelo que diz a linha dela: `true`
    /// quando o nome sai seguido da marca.
    fn marked_for_review(prompt: &WavePrompt) -> bool {
        let review = crate::platform::i18n::translate("prompt.skill.stale", Locale::PtBr);
        prompt.text.contains(&format!("/somar/SKILL.md` ({review})"))
    }

    /// A skill cujo arquivo citado teve commit depois da data do arquivo
    /// dela sai marcada como a revisar, também quando a citação traz só o fim
    /// do caminho e a linha: a data vem do próprio arquivo, sem precisar de
    /// nenhum registro da skill na spec.
    #[test]
    fn a_skill_whose_cited_file_changed_after_it_is_marked_for_review() {
        let written = 1_789_000_000;
        let built = request_with_skill_written_at(written, written + 3_600, true, "src/exemplo.rs:12");
        assert!(built.bad_skills.is_empty(), "{:?}", built.bad_skills);
        assert_eq!(built.stale_skills, ["somar"]);
        assert!(marked_for_review(&built), "{}", built.text);
    }

    /// A skill mais nova que os commits de todos os arquivos que ela cita sai
    /// sem a marca; sem o mapa do projeto, não há histórico e também não há
    /// marca.
    #[test]
    fn a_skill_newer_than_its_cited_files_or_without_a_map_is_not_marked() {
        let written = 1_789_000_000;
        for (moved, with_map) in [(written - 3_600, true), (written + 3_600, false)] {
            let built = request_with_skill_written_at(written, moved, with_map, "apps/rt/src/exemplo.rs");
            assert!(built.bad_skills.is_empty(), "{:?}", built.bad_skills);
            assert!(built.stale_skills.is_empty(), "commit em {moved}, mapa {with_map}: {:?}", built.stale_skills);
            assert!(!marked_for_review(&built), "{}", built.text);
            assert!(built.text.contains("skills/somar/SKILL.md"), "{}", built.text);
        }
    }

    /// O pedido da onda que sai agora traz a cópia que a rodada escolheu; o da
    /// onda em andamento, a cópia gravada no envio dela, e o da onda que não
    /// está fora não fala de cópia. O envio antigo, que gravava também uma
    /// pasta de compilação à parte, ainda é lido: só a cópia dele vale.
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
        let chosen = WaveCopy { path: "/c/dois".into(), reused: None };
        let flight = Flight { running: [1, 2].into(), copies: [(2, chosen)].into(), ..Flight::default() };
        let built = prompts(root, "teste", &log, Locale::PtBr, &flight, &log.planned_waves());
        assert_eq!(recorded_copy(&log, 1), Some(WaveCopy { path: "/c/um".into(), reused: None }));
        assert!(built[0].text.contains("`/c/um`") && !built[0].text.contains("/t/a"), "{}", built[0].text);
        assert!(built[1].text.contains("`/c/dois`"), "{}", built[1].text);
        assert!(built[0].text.contains(&format!("--root {} --spec teste", shown(root))), "{}", built[0].text);

        let still = prompts(root, "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        assert!(!still[0].text.contains("/c/um"), "{}", still[0].text);
        assert!(!still[0].text.contains("--root"), "sem cópia, o agente lê a spec de onde está: {}", still[0].text);
    }

    /// Os pedidos que a rodada monta — o da onda e o da revisão final — num
    /// projeto sem mapa, num só com parte Node, num com uma parte Node e uma
    /// Rust e num só Rust: nenhum cita pasta de compilação à parte, o Cargo
    /// ou a variável dele. A compilação mora dentro da cópia, e a cópia — a
    /// da onda ou a limpa, do revisor — aparece em todos. O envio antigo com a pasta gravada não a traz de
    /// volta.
    #[test]
    fn no_request_names_a_build_folder_in_any_project() {
        let node = json!({"name": "web", "dir": "web", "kind": "npm", "code_files": 3});
        let rust = json!({"name": "api", "dir": "api", "kind": "cargo", "code_files": 3});
        for map in [None, Some(json!([node])), Some(json!([node, rust])), Some(json!([rust]))] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            if let Some(projects) = &map {
                write_map(root, &json!({"projects": projects}));
            }
            let folder = shown(&root.join("target").join("copias").join("a"));
            let copy = shown(&slot_path(root, "teste", 0));
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
                let built = prompts(root, "teste", &log, lang, &flight, &log.planned_waves());
                let last = final_review(root, "teste", &log, lang).text;
                let clean = shown(&check_copy_path(root, "teste"));
                for (what, text, copy) in [("wave", &built[0].text, &copy), ("final", &last, &clean)] {
                    assert!(text.contains(&format!("`{copy}`")), "{map:?} {lang:?} {what}: {text}");
                    for word in ["CARGO_TARGET_DIR", "Cargo", "target/copias", &folder] {
                        assert!(!text.contains(word), "{map:?} {lang:?} {what} cita {word}: {text}");
                    }
                }
            }
        }
    }

    /// O revisor final trabalha na cópia limpa da spec, fora das vagas das
    /// ondas, qualquer que seja a vaga do último envio. As vagas das ondas
    /// têm nome de letra e, depois da última, de número.
    #[test]
    fn the_final_review_uses_the_clean_copy() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let send = json!({"wave": 1, "role": "wave", "text": "p", "lines": 1, "chars": 1, "items": [1],
            "mustard": "0", "author": "binary", "copy": shown(&slot_path(root, "teste", 1))});
        let log = log_of(&[("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})), ("send", send)]);
        let clean = check_copy_path(root, "teste");
        assert_eq!(clean, copies_dir(root).join("teste").join("check"));
        assert!(final_review(root, "teste", &log, Locale::PtBr).text.contains(&format!("`{}`", shown(&clean))));
        assert_eq!(slot_name(0), "a");
        assert_eq!(slot_name(25), "z");
        assert_eq!(slot_name(26), "27");
        assert_eq!(slot_path(root, "teste", 1), copies_dir(root).join("teste").join("b"));
    }

    /// A pasta do programa compilado leva a mesma chave por projeto da pasta
    /// das cópias — o nome da pasta do checkout e o código do caminho dele —,
    /// separa dois projetos de mesmo nome e nunca é a pasta das cópias nem
    /// fica dentro do projeto.
    #[test]
    fn the_build_folder_carries_the_same_project_key_as_the_copies() {
        let place = tempdir().unwrap();
        let named = |parent: &str| {
            let root = place.path().join(parent).join("mustard");
            std::fs::create_dir_all(&root).unwrap();
            root
        };
        let (one, other) = (named("um"), named("outro"));
        assert_eq!(development_build_dir(&one).file_name(), copies_dir(&one).file_name());
        assert!(development_build_dir(&one).file_name().unwrap().to_string_lossy().starts_with("mustard-"));
        assert_ne!(development_build_dir(&one), development_build_dir(&other), "dois projetos de mesmo nome ficam separados");
        assert_ne!(development_build_dir(&one), copies_dir(&one));
        assert!(!development_build_dir(&one).starts_with(&one), "o fechamento só apaga o que está dentro do projeto");
        assert_eq!(development_build_dir(&one), development_build_dir(&one.join(".")), "o caminho é lido já resolvido");
    }

    /// O prefixo de caminho literal sai do caminho de disco e do de
    /// compartilhamento, com as barras de cada texto; o do volume e o caminho
    /// comum ficam como vieram. Texto puro: a mesma resposta em todo sistema.
    #[test]
    fn the_literal_prefix_leaves_a_drive_and_a_share_and_keeps_a_volume() {
        assert_eq!(without_verbatim(r"\\?\C:\Users\runner\proj").as_deref(), Some(r"C:\Users\runner\proj"));
        assert_eq!(without_verbatim("//?/C:/Users/runner/proj").as_deref(), Some("C:/Users/runner/proj"));
        assert_eq!(without_verbatim(r"\\.\D:\x").as_deref(), Some(r"D:\x"), "o prefixo de aparelho sai igual");
        assert_eq!(without_verbatim(r"\\?\UNC\srv\share\x").as_deref(), Some(r"\\srv\share\x"));
        assert_eq!(without_verbatim("//?/UNC/srv/share/x").as_deref(), Some("//srv/share/x"));
        assert_eq!(without_verbatim(r"\\?\Volume{0b1c2d3e-0000-0000-0000-100000000000}\x"), None, "o volume não tem outra grafia");
        assert_eq!(without_verbatim(r"C:\Users\runner\proj"), None);
        assert_eq!(without_verbatim("/home/runner/proj"), None);
        assert_eq!(without_verbatim(r"\\srv\share\x"), None, "o compartilhamento sem prefixo fica");
    }

    /// A obra da revisão de volta: três requisitos acordados, um critério, a
    /// onda e o commit dela, e o veredito final que reprovou um requisito.
    /// Depois dele, um commit novo, a versão nova de outro requisito, um
    /// requisito novo e um critério novo. `verdict` troca o veredito: `None`
    /// tira ele e tudo o que veio depois; `Some("approved")` o grava
    /// aprovando, com todo requisito atendido.
    fn reviewed_again(root: &Path, verdict: Option<&str>) -> SpecLog {
        let repo = root.file_name().unwrap().to_string_lossy().to_string();
        let commit = |sha: &str| ("commit", json!({"sha": sha, "title": "t", "waves": [1], "files": ["src/a.rs"], "repo": repo}));
        let mut events = vec![
            ("rule", json!({"text": "Atendida antes", "keys": ["a"], "example": "e", "applies_to": {"files": ["**"]}})),
            ("decision", json!({"text": "Reprovada", "keys": ["b"], "why": "w"})),
            ("decision", json!({"text": "Regravada", "keys": ["c"], "why": "w"})),
            ("criterion", json!({"when": "a onda roda", "then": "passa", "proof": "true"})),
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [4], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "src/a.rs"}]})),
            commit("aaa1111"),
        ];
        let Some(result) = verdict else {
            return log_of(&events);
        };
        let second = if result == "approved" { json!({"item": 2, "met": true}) } else { json!({"item": 2, "met": false, "text": "falta"}) };
        let agreed = json!([{"item": 1, "met": true}, second, {"item": 3, "met": true}]);
        events.push(("verdict", json!({"final": true, "result": result, "text": "v", "agreed": agreed})));
        events.push(commit("bbb2222"));
        events.push(("decision", json!({"text": "Regravada de novo", "keys": ["c"], "why": "w", "replaces": 3})));
        events.push(("rule", json!({"text": "Nova", "keys": ["d"], "example": "e", "applies_to": {"files": ["**"]}})));
        events.push(("criterion", json!({"when": "a volta roda", "then": "passa", "proof": "true"})));
        log_of(&events)
    }

    /// As linhas de lista da parte `heading` de um pedido; vazia quando a
    /// parte não existe.
    fn part_lines(text: &str, heading: &str) -> Vec<String> {
        let Some((_, rest)) = text.split_once(&format!("## {heading}\n")) else {
            return Vec::new();
        };
        let part = rest.split("\n## ").next().unwrap_or_default();
        part.lines().filter(|line| line.starts_with("- ")).map(str::to_string).collect()
    }

    /// Os códigos das linhas de uma parte do pedido, na ordem: a linha de um
    /// item abre com o tipo por extenso e traz o código logo depois.
    fn line_codes(lines: Vec<String>) -> Vec<String> {
        lines.iter().filter_map(|line| line.find("MSTD-").map(|at| line[at..].split(' ').next().unwrap_or_default().to_string())).collect()
    }

    /// A revisão que volta depois de um veredito final que reprovou traz a
    /// parte do que mudou desde ele: o veredito, só os commits gravados
    /// depois dele, o requisito que ele deu como não atendido, o regravado e
    /// o novo depois dele e o critério novo. O resto não entra nessa parte,
    /// mas todos os requisitos acordados continuam no pedido, porque o
    /// veredito responde por cada um. A que vem depois de uma aprovação traz
    /// a mesma parte, sem o que a aprovação conferiu e não mudou em parte
    /// nenhuma. A primeira revisão confere a obra inteira, sem essa parte.
    /// Nenhuma manda rodar a suíte: dizem que ela passou no fechamento, no
    /// commit da cópia, e mandam rodar só os testes em volta de cada corte.
    /// Nos dois idiomas.
    #[test]
    fn a_review_after_a_rejection_lists_only_what_changed_since_the_verdict() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let config = json!({"buildCommand": "make", "testCommand": "make test"});
        std::fs::write(root.join("mustard.json"), config.to_string()).unwrap();
        let again = reviewed_again(root, Some("rejected"));
        let codes = again.codes();
        let code = |id: u64| codes[&id].clone();
        for lang in [Locale::PtBr, Locale::EnUs] {
            let t = |key: &str| crate::platform::i18n::translate(key, lang);
            let since = t("prompt.part.since_verdict");
            // O comando de testar aparece uma vez só, na linha que diz que a
            // suíte passou no fechamento: nenhum outro trecho manda rodá-la.
            let whole_suite_once = |text: &str| text.matches("make test").count() == 1;
            let build = format!("- {}", t("prompt.execution.build").replace("{command}", "make"));

            let last = final_review(root, "teste", &again, lang).text;
            assert_eq!(line_codes(part_lines(&last, since)), [code(8), code(9), code(2), code(10), code(11), code(12)], "{lang:?}: {last}");
            assert!(!part_lines(&last, since).concat().contains(&code(7)), "o commit de antes fica fora: {last}");
            assert_eq!(
                line_codes(part_lines(&last, t("prompt.part.agreed"))),
                [code(1), code(2), code(10), code(11)],
                "o pedido ainda traz todos os requisitos acordados: {last}"
            );
            assert!(last.contains(t("prompt.final.look_again")) && !last.contains(t("prompt.final.look")), "{last}");
            let suite = t("prompt.review.suite").replace("{command}", "make test").replace("{commit}", "bbb2222");
            assert!(last.contains(&format!("- {suite}")) && last.contains(&build), "{lang:?}: {last}");
            assert!(whole_suite_once(&last), "a revisão não roda a suíte inteira: {last}");

            let approved = reviewed_again(root, Some("approved"));
            let after = final_review(root, "teste", &approved, lang).text;
            assert_eq!(line_codes(part_lines(&after, since)), [code(8), code(9), code(10), code(11), code(12)], "{lang:?}: {after}");
            assert_eq!(line_codes(part_lines(&after, t("prompt.part.agreed"))), [code(10), code(11)], "{after}");
            for old in [1, 2, 7] {
                assert!(!after.contains(&code(old)), "o que a aprovação conferiu fica fora: {after}");
            }
            assert!(after.contains(t("prompt.final.look_since_approval")) && !after.contains(t("prompt.final.look_again")), "{after}");
            assert!(whole_suite_once(&after), "a revisão não roda a suíte inteira: {after}");

            let first = final_review(root, "teste", &reviewed_again(root, None), lang).text;
            assert!(part_lines(&first, since).is_empty() && !first.contains(&format!("## {since}")), "{first}");
            assert!(first.contains(t("prompt.final.look")) && !first.contains(t("prompt.final.look_again")), "{first}");
            assert!(whole_suite_once(&first), "a revisão não roda a suíte inteira: {first}");
            let suite = t("prompt.review.suite").replace("{command}", "make test").replace("{commit}", "aaa1111");
            assert!(first.contains(&format!("- {suite}")) && first.contains(&build), "{first}");
        }
    }

    /// O que o pedido da revisão lista sai só da spec, sem montar o pedido: a
    /// lista que o painel, a rodada e o fechamento conferem é a mesma que o
    /// pedido montado no projeto grava, com as regras do projeto e a cópia do
    /// revisor no disco, na primeira revisão e na de volta.
    #[test]
    fn the_items_the_final_review_lists_come_from_the_spec_alone() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("CLAUDE.md"), "Nunca duplicar tipos.\n").unwrap();
        for log in [reviewed_again(root, None), reviewed_again(root, Some("rejected"))] {
            let request = final_review(root, "teste", &log, Locale::PtBr);
            assert!(request.text.contains("Nunca duplicar tipos."), "{}", request.text);
            assert!(request.listed.len() > 3, "{}", request.text);
            assert_eq!(final_review_listed(&log), request.listed, "{}", request.text);
        }
    }

    /// O que o fechamento grava no envio da revisão como leitura obrigatória
    /// é o que o pedido imprime: o código de cada linha de item, na ordem, uma
    /// vez só mesmo quando o requisito sai em duas partes — o novo depois do
    /// veredito sai em "o que mudou" e em "requisitos acordados".
    #[test]
    fn the_review_reading_list_is_the_codes_of_the_lines_the_request_prints() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let again = reviewed_again(root, Some("rejected"));
        for lang in [Locale::PtBr, Locale::EnUs] {
            let review = final_review(root, "teste", &again, lang);
            let all: Vec<String> = review.text.lines().filter(|line| line.starts_with("- ")).map(str::to_string).collect();
            let mut printed: Vec<String> = Vec::new();
            for code in line_codes(all) {
                if !printed.contains(&code) {
                    printed.push(code);
                }
            }
            assert!(printed.len() > 6, "{}", review.text);
            assert_eq!(review.listed, printed, "{lang:?}: {}", review.text);
        }
    }

    /// Até onde vai a obra de duas ondas de [`two_waves_reviewed`].
    enum Reviewed {
        /// Nenhum veredito final ainda.
        First,
        /// O veredito final reprovou a onda 1, e o conserto dela entrou.
        WaveRejected,
        /// O veredito final reprovou a obra sem apontar onda.
        WorkRejected,
        /// Depois do conserto da onda 1, um veredito final aprovou a obra.
        ApprovedAfter,
    }

    /// Uma obra de duas ondas entregues e comitadas, com os vereditos finais
    /// que `reviewed` pede. Cada reprovação vem com o conserto entregue e
    /// comitado depois dela.
    fn two_waves_reviewed(reviewed: &Reviewed) -> SpecLog {
        let commit = |sha: &str, waves: Value| ("commit", json!({"sha": sha, "title": "t", "waves": waves, "files": ["src/a.rs"]}));
        let mut events = vec![
            ("rule", json!({"text": "Do projeto", "keys": ["a"], "example": "e", "applies_to": {"files": ["**"]}})),
            ("criterion", json!({"when": "a onda roda", "then": "passa", "proof": "true"})),
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [2], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "src/a.rs"}]})),
            ("wave", json!({"n": 2, "text": "A outra", "criteria": [2], "done_when": "passa"})),
            ("task", json!({"wave": 2, "text": "Subtrair", "files": [{"path": "src/b.rs"}]})),
            ("delivered", json!({"wave": 1, "text": "Feito", "files": ["src/a.rs"]})),
            ("delivered", json!({"wave": 2, "text": "Feita", "files": ["src/b.rs"]})),
            commit("aaa1111", json!([1, 2])),
        ];
        if matches!(reviewed, Reviewed::First) {
            return log_of(&events);
        }
        let mut rejected = json!({"final": true, "result": "rejected", "text": "Falta o teste",
                                  "agreed": [{"item": 1, "met": false, "text": "falta"}]});
        if !matches!(reviewed, Reviewed::WorkRejected) {
            rejected["wave"] = json!(1);
        }
        events.push(("verdict", rejected));
        events.push(("delivered", json!({"wave": 1, "text": "Consertado", "files": ["src/a.rs"]})));
        events.push(commit("bbb2222", json!([1])));
        if matches!(reviewed, Reviewed::ApprovedAfter) {
            let approved = json!({"final": true, "result": "approved", "text": "Pronta", "agreed": [{"item": 1, "met": true}]});
            events.push(("verdict", approved));
        }
        log_of(&events)
    }

    /// A revisão de volta de um veredito que reprovou uma onda cita esse
    /// veredito uma vez só, na parte do que mudou, e nenhuma outra parte o
    /// repete nem manda conferir o conserto à parte: o pedido tem as mesmas
    /// partes da revisão de volta de uma obra inteira. Ela diz, no parágrafo
    /// do que olhar, que a parte das ondas traz só as tarefas dessa onda, e a
    /// parte traz só elas. A primeira
    /// revisão não diz o recorte e traz as duas ondas. O veredito que reprovou a obra sem
    /// apontar onda traz as duas ondas na revisão de volta. Um veredito final
    /// que aprovou depois da reprovação de uma onda tira o recorte e a frase
    /// dele: a revisão seguinte confere o que mudou desde a aprovação, e as
    /// tarefas que ela conferiu ficam fora. Nos dois idiomas.
    #[test]
    fn a_review_after_a_rejected_wave_says_it_carries_only_that_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for lang in [Locale::PtBr, Locale::EnUs] {
            let t = |key: &str| crate::platform::i18n::translate(key, lang);
            let scope_phrase = match lang {
                Locale::PtBr => {
                    "Se ele reprovou uma onda, a parte das ondas traz só as tarefas dessa onda; senão, \
                     traz as de todas as ondas."
                }
                Locale::EnUs => {
                    "If it rejected one wave, the waves part carries only that wave's tasks; otherwise, \
                     it carries every wave's."
                }
            };
            // Se a parte das ondas traz a tarefa da onda 1 e a da onda 2.
            let wave_codes = |log: &SpecLog, text: &str| -> Vec<bool> {
                let codes = log.codes();
                let listed = part_lines(text, t("prompt.part.waves")).concat();
                [4, 6].iter().map(|id| listed.contains(codes[id].as_str())).collect()
            };

            let log = two_waves_reviewed(&Reviewed::WaveRejected);
            let again = final_review(root, "teste", &log, lang).text;
            assert!(again.contains(scope_phrase), "{lang:?}: a revisão de volta diz o recorte: {again}");
            assert_eq!(again.matches(scope_phrase).count(), 1, "o recorte é dito uma vez só: {again}");
            assert_eq!(wave_codes(&log, &again), [true, false], "só a onda reprovada: {again}");
            let codes = log.codes();
            let code = |id: u64| codes[&id].clone();
            assert_eq!(again.matches(code(10).as_str()).count(), 1, "o veredito aparece uma vez só: {again}");
            assert_eq!(line_codes(part_lines(&again, t("prompt.part.since_verdict"))).first(), Some(&code(10)), "{again}");
            let headings: Vec<&str> = again.lines().filter_map(|line| line.strip_prefix("## ")).collect();
            let expected: Vec<&str> = [
                "prompt.part.read",
                "prompt.part.since_verdict",
                "prompt.part.waves",
                "prompt.part.agreed",
                "prompt.part.criteria",
                "prompt.part.execution",
            ]
            .into_iter()
            .map(t)
            .collect();
            assert_eq!(headings, expected, "nenhuma parte de conserto à parte: {again}");
            assert!(again.contains(t("prompt.final.look_again")), "{lang:?}: {again}");
            assert_eq!(line_codes(part_lines(&again, t("prompt.part.waves"))), [code(4)], "{again}");

            let log = two_waves_reviewed(&Reviewed::First);
            let first = final_review(root, "teste", &log, lang).text;
            assert!(!first.contains(scope_phrase), "{lang:?}: a primeira revisão não diz recorte: {first}");
            assert_eq!(wave_codes(&log, &first), [true, true], "{first}");

            let log = two_waves_reviewed(&Reviewed::WorkRejected);
            let whole = final_review(root, "teste", &log, lang).text;
            assert!(whole.contains(t("prompt.final.look_again")), "{lang:?}: {whole}");
            assert_eq!(wave_codes(&log, &whole), [true, true], "a obra reprovada traz todas as ondas: {whole}");

            let log = two_waves_reviewed(&Reviewed::ApprovedAfter);
            let after = final_review(root, "teste", &log, lang).text;
            assert!(after.contains(t("prompt.final.look_since_approval")) && !after.contains(scope_phrase), "{lang:?}: {after}");
            assert_eq!(wave_codes(&log, &after), [false, false], "a aprovação já conferiu as duas ondas: {after}");
        }
    }

    /// A revisão que volta depois de um conserto confere só a cadeia dele. O
    /// pedido manda conferir a regra, cada lugar e a prova, e não manda olhar
    /// o encaixe no resto da obra. A parte do que mudou traz a tarefa do
    /// conserto, que tem passo depois do veredito, com a cadeia logo abaixo:
    /// a regra pelo código e título, os lugares e a prova do critério que ela
    /// cobre. A tarefa terminada antes do veredito não entra em parte
    /// nenhuma. Depois de uma aprovação, o ajuste traz a cadeia dele, com a
    /// regra nova pelo texto, e o conserto que ela aprovou fica de fora. Nos
    /// dois idiomas.
    #[test]
    fn the_review_after_a_fix_checks_the_rule_its_places_and_its_proof_only() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let commit = |sha: &str, wave: u64| ("commit", json!({"sha": sha, "title": "t", "waves": [wave], "files": ["src/a.rs"]}));
        let mut events = vec![
            ("rule", json!({"title": "A soma guarda o sinal", "text": "A soma guarda o sinal.", "keys": ["a"], "example": "e", "applies_to": {"files": ["**"]}})),
            ("criterion", json!({"when": "a soma roda", "then": "o sinal fica", "proof": "cargo test soma_keeps_sign", "form": "ubiquitous"})),
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [2], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "src/a.rs"}], "covers": [2]})),
            ("step", json!({"wave": 1, "item": 4, "text": "Pronta."})),
            commit("aaa1111", 1),
            ("verdict", json!({"final": true, "result": "rejected", "text": "O sinal some.", "agreed": [{"item": 1, "met": false, "text": "falta"}]})),
            ("wave", json!({"n": 2, "text": "O conserto", "criteria": [2], "done_when": "passa"})),
            ("task", json!({"wave": 2, "text": "Guardar o sinal", "files": [{"path": "src/a.rs"}], "rule": "MSTD-RULE-0001",
                "places": ["src/a.rs:3", "soma"], "covers": [1, 2]})),
            ("step", json!({"wave": 2, "item": 9, "text": "Pronta."})),
            commit("bbb2222", 2),
        ];
        let fixed = log_of(&events);
        events.push(("verdict", json!({"final": true, "result": "approved", "text": "Pronta.", "agreed": [{"item": 1, "met": true}]})));
        events.push(("task", json!({"wave": 2, "text": "Mostrar o total", "files": [{"path": "src/b.rs"}], "rule": "O total cabe no visor.",
            "places": ["src/b.rs"]})));
        events.push(("step", json!({"wave": 2, "item": 13, "text": "Pronta."})));
        let adjusted = log_of(&events);
        let codes = fixed.codes();
        let code = |id: u64| codes[&id].clone();
        // A linha logo abaixo da primeira linha de item que cita `code`.
        let below = |text: &str, code: &str| -> String {
            let mut lines = text.lines();
            lines.find(|line| line.starts_with("- ") && line.contains(code));
            lines.next().unwrap_or_default().to_string()
        };
        for (lang, old) in [(Locale::PtBr, "se encaixa no resto da obra"), (Locale::EnUs, "fits the rest of the work")] {
            let t = |key: &str| crate::platform::i18n::translate(key, lang);

            let again = final_review(root, "teste", &fixed, lang).text;
            assert!(again.contains(t("prompt.final.look_again")) && !again.contains(old), "{lang:?}: {again}");
            assert_eq!(line_codes(part_lines(&again, t("prompt.part.since_verdict"))), [code(7), code(11), code(1), code(9)], "{lang:?}: {again}");
            let chain = format!(
                "  {} {} {}",
                t("prompt.final.chain.rule").replace("{rule}", &format!("{} — A soma guarda o sinal", code(1))),
                t("prompt.final.chain.places").replace("{places}", "`src/a.rs:3`, `soma`"),
                t("prompt.final.chain.proof").replace("{proofs}", "`cargo test soma_keeps_sign`"),
            );
            assert_eq!(below(&again, &code(9)), chain, "{lang:?}: {again}");
            assert!(!again.contains(&code(4)), "a tarefa terminada antes do veredito fica fora: {again}");

            let after = final_review(root, "teste", &adjusted, lang).text;
            assert!(after.contains(t("prompt.final.look_since_approval")) && !after.contains(old), "{lang:?}: {after}");
            let rule = t("prompt.final.chain.rule").replace("{rule}", "O total cabe no visor");
            assert_eq!(below(&after, &adjusted.codes()[&13]), format!("  {rule} {}", t("prompt.final.chain.places").replace("{places}", "`src/b.rs`")), "{after}");
            assert!(!after.contains(&code(9)), "o conserto que a aprovação conferiu fica fora: {after}");
        }
    }

    /// Num projeto que declara o preparo (`npm ci`) e os arquivos locais, o
    /// pedido da onda e o do revisor mandam rodar o preparo dentro da cópia
    /// antes de compilar — a linha vem antes da de compilar — e devolver ao
    /// commit, pelo `git checkout`, o arquivo versionado que ele mudar, salvo
    /// o que a tarefa declara. Nenhum dos dois lista os arquivos locais: a
    /// cópia da onda os recebe da rodada, e a do revisor é a cópia limpa.
    /// Sem comando — chave ausente ou vazia —, nenhum pedido traz essas
    /// linhas. Nos dois idiomas.
    #[test]
    fn the_wave_requests_cite_the_prepare_command_and_no_local_file() {
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "src/a.rs"}]})),
        ]);
        let declared = json!({"buildCommand": "npm run build", "prepareCommand": "npm ci",
            "localFiles": [".env", "apps/api/.env.local", "../fora.env", "/etc/fora.env"]});
        let empty = json!({"buildCommand": "npm run build", "prepareCommand": "  ", "localFiles": []});
        let absent = json!({"buildCommand": "npm run build"});
        for (config, cites) in [(declared, true), (empty, false), (absent, false)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            std::fs::write(root.join("mustard.json"), config.to_string()).unwrap();
            let copy = WaveCopy { path: "/c/um".into(), reused: None };
            let flight = Flight { running: [1].into(), copies: [(1, copy)].into(), ..Flight::default() };
            for lang in [Locale::PtBr, Locale::EnUs] {
                let t = |key: &str| crate::platform::i18n::translate(key, lang);
                let new_copy = t("prompt.execution.prepare_new").replace("{command}", "npm ci");
                let review = t("prompt.execution.prepare").replace("{command}", "npm ci");
                let build = t("prompt.execution.build").replace("{command}", "npm run build");
                let wave = prompts(root, "teste", &log, lang, &flight, &log.planned_waves()).remove(0).text;
                let last = final_review(root, "teste", &log, lang).text;
                for (what, text, prepare) in [("wave", &wave, &new_copy), ("final", &last, &review)] {
                    assert_eq!(text.contains(prepare.as_str()), cites, "{config} {lang:?} {what}: {text}");
                    assert_eq!(text.contains("npm ci"), cites, "{config} {lang:?} {what}: {text}");
                    assert_eq!(text.contains("git checkout --"), cites, "{config} {lang:?} {what}: {text}");
                    if cites {
                        let (at, built_at) = (text.find(prepare.as_str()).unwrap(), text.find(&build).unwrap());
                        assert!(at < built_at, "o preparo vem antes de compilar: {lang:?} {what}: {text}");
                    }
                    assert!(!text.contains("fora.env"), "{lang:?} {what}: {text}");
                }
                assert!(!last.contains(".env"), "a cópia limpa do revisor nunca recebe os arquivos locais: {lang:?}: {last}");
                assert!(!wave.contains(".env"), "a cópia da onda já recebe os arquivos da rodada: {lang:?}: {wave}");
            }
        }
    }

    /// A vaga nova pede o preparo sempre. A reaproveitada lista os arquivos
    /// que mudaram desde o último uso dela e pede o preparo só se um deles
    /// declara dependências, sem dizer quais são: o agente julga pela lista.
    /// Sem nada mudado, ela manda não preparar de novo. A lista longa para no
    /// teto e diz quantos faltam, com o comando que lista todos.
    #[test]
    fn a_reused_copy_lists_the_changed_files_and_prepares_only_when_needed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), json!({"prepareCommand": "npm ci"}).to_string()).unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "src/a.rs"}]})),
        ]);
        let with = |reused: Option<Reuse>| {
            let copy = WaveCopy { path: "/c/a".into(), reused };
            let flight = Flight { running: [1].into(), copies: [(1, copy)].into(), ..Flight::default() };
            prompts(root, "teste", &log, Locale::PtBr, &flight, &log.planned_waves()).remove(0).text
        };
        let t = |key: &str| crate::platform::i18n::translate(key, Locale::PtBr).replace("{command}", "npm ci");
        let reuse = |changed: Vec<String>| Some(Reuse { since: "abc1234".into(), changed });

        let fresh = with(None);
        assert!(fresh.contains(&t("prompt.execution.prepare_new")), "{fresh}");

        let listed = with(reuse(vec!["package.json".into(), "src/a.ts".into()]));
        let line = t("prompt.execution.prepare_reused").replace("{files}", "`package.json`, `src/a.ts`");
        assert!(listed.contains(&line), "{listed}");
        assert!(!listed.contains(&t("prompt.execution.prepare_new")), "{listed}");

        let same = with(reuse(Vec::new()));
        assert!(same.contains(&t("prompt.execution.prepare_same")), "{same}");

        let many: Vec<String> = (0..45).map(|n| format!("src/{n}.ts")).collect();
        let long = with(reuse(many));
        let more = crate::platform::i18n::translate("prompt.execution.prepare_more", Locale::PtBr)
            .replace("{n}", "5")
            .replace("{diff}", "git diff --name-only abc1234 HEAD");
        assert!(long.contains("`src/39.ts`, ") && long.contains(&more), "{long}");
        assert!(!long.contains("`src/40.ts`"), "{long}");
    }

    /// A skill que a tarefa nomeia e que não está no disco é recusada, com o
    /// caminho em que ela devia estar.
    #[test]
    fn a_named_skill_that_is_not_on_disk_is_refused_with_the_path() {
        let dir = tempdir().unwrap();
        let built = prompts(dir.path(), "teste", &plan_log(), Locale::PtBr, &Flight::default(), &plan_log().planned_waves());
        assert_eq!(built[0].bad_skills.len(), 1);
        assert_eq!(built[0].bad_skills[0].1.reason(), "skill-missing-path");
        assert!(built[0].bad_skills[0].1.message(Locale::PtBr).contains("somar"));
    }

    /// A onda que continua o resumo de uma onda que parou o lista pelo código,
    /// em primeiro lugar, na lista de leitura da entrega e no bloco do começo
    /// do pedido; o texto do resumo não entra, e a onda que não o continua
    /// não o lista.
    #[test]
    fn the_wave_that_continues_a_summary_lists_it_first_and_the_text_never_enters() {
        let dir = tempdir().unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "Uma.", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar o dia.", "files": [{"path": "src/a.rs"}]})),
            ("delivered", json!({"wave": 1, "text": "Parei no meio da soma.", "undone": ["MSTD-TASK-0002"]})),
            ("wave", json!({"n": 2, "text": "Duas.", "criteria": [], "done_when": "passa", "summary": 3})),
            ("task", json!({"wave": 2, "text": "Mostrar o total.", "files": [{"path": "src/b.rs"}]})),
        ]);
        let code = log.codes()[&3].clone();
        let built = prompts(dir.path(), "teste", &log, Locale::PtBr, &Flight::default(), &log.planned_waves());
        let (first, second) = (&built[0], &built[1]);
        assert_eq!((first.wave, second.wave), (1, 2));

        assert_eq!(second.listed.first(), Some(&code), "{:?}", second.listed);
        assert!(second.text.contains(&format!("run read item-{code} --spec teste")), "{}", second.text);
        assert!(!second.text.contains("Parei no meio da soma."), "{}", second.text);
        assert!(!first.listed.contains(&code), "the wave that continues nothing does not list it: {:?}", first.listed);
    }
    #[test]
    fn prepared_context_checks_the_actual_wave_copy_and_reuses_only_unchanged_components() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        let copy = dir.path().join("wave-copy");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn Alpha() { let value = 1; }\n").unwrap();
        std::fs::write(root.join("src/b.rs"), "fn Beta() { let value = 2; }\n").unwrap();
        let git = |tree: &Path, args: &[&str]| {
            let run = crate::platform::git::run(tree, args);
            assert!(run.ok, "git {args:?}: {}", run.stderr);
            run.stdout.trim().to_string()
        };
        git(&root, &["init", "-q"]);
        git(&root, &["add", "src"]);
        git(&root, &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-qm", "seed"]);
        git(&root, &["worktree", "add", "--detach", copy.to_str().unwrap()]);
        let save = |a_line: u64| {
            write_map(&root, &json!({"modules":[
                {"path":"src/a.rs","blob":git(&copy, &["hash-object","src/a.rs"]),"declarations":[{"kind":"function","name":"Alpha","line":a_line,"end_line":a_line}]},
                {"path":"src/b.rs","blob":git(&copy, &["hash-object","src/b.rs"]),"declarations":[{"kind":"function","name":"Beta","line":1,"end_line":1}]}
            ]}));
        };
        save(1);
        let log = log_of(&[
            ("wave", json!({"n":1,"text":"Deliver", "criteria":[],"done_when":"done"})),
            ("task", json!({"wave":1,"text":"Update Alpha and Beta","files":[{"path":"src/a.rs"},{"path":"src/b.rs"}],"must_read":["src/a.rs#Alpha","src/b.rs#Beta"]})),
        ]);
        let mut flight = Flight::default();
        flight.copies.insert(1, WaveCopy { path: shown(&copy), reused: None });
        let build = || prompts(&root, "prepared", &log, Locale::PtBr, &flight, &log.planned_waves()).remove(0);
        let first = build();
        assert!(first.text.contains("fn Alpha() { let value = 1; }"));
        assert!(first.text.contains("fn Beta() { let value = 2; }"));
        let cache = root.join(".claude/mustard/prepared-context");
        let components = || {
            std::fs::read_dir(&cache)
                .unwrap()
                .map(|entry| {
                    let path = entry.unwrap().path();
                    (path.clone(), std::fs::read(path).unwrap())
                })
                .collect::<BTreeMap<_, _>>()
        };
        for entry in std::fs::read_dir(&cache).unwrap() {
            let path = entry.unwrap().path();
            let mut stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            stored["reuse_probe"] = json!("retained only on a real cache hit");
            std::fs::write(path, stored.to_string()).unwrap();
        }
        let before = components();
        assert_eq!(before.len(), 2);
        assert_eq!(build(), first, "same input produces the same canonical request");
        assert_eq!(components(), before);
        std::fs::write(copy.join("src/a.rs"), "// inserted\nfn Alpha() { let value = 9; }\n").unwrap();
        let changed = build();
        assert!(!changed.text.contains("fn Alpha() { let value = 1; }"), "a dirty copy never receives the main checkout's old source");
        assert!(changed.text.contains("map-source-unverified"));
        assert!(!changed.text.contains("linhas 1-1 de `Alpha`"));
        assert!(changed.text.contains("fn Beta() { let value = 2; }"));
        assert_eq!(changed.listed, first.listed, "prepared excerpts never fabricate or erase canonical read requirements");
        save(2);
        let refreshed = build();
        assert!(refreshed.text.contains("fn Alpha() { let value = 9; }"));
        assert!(refreshed.text.contains("src/a.rs:2-2#Alpha"));
        let after = components();
        assert_eq!(before.iter().filter(|(path, bytes)| after.get(*path) == Some(*bytes)).count(), 1, "only the affected component changes");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn Alpha() { let value = 1; }\n");
    }

}
