//! A medida do padrão por tarefa: o pedido da onda montado com o padrão e sem
//! ele, o que o bloco custa em tokens sob cada tarefa e a régua de qualidade
//! sobre o que o bloco mostra.
//!
//! Os dois lados do pedido saem da mesma função ([`prompts_reading`]), do
//! mesmo mapa e da mesma spec. A única diferença é que o lado "sem" lê um
//! mapa que recusa a pergunta de que o padrão nasce ([`Need::Examples`]):
//! nada mais muda, e a conta de caracteres de cada onda confere isso, porque
//! a diferença entre os dois pedidos tem de ser exatamente a soma dos blocos
//! do padrão. Sem agente e sem Jev: só a spec e o mapa que já existem.
//!
//! A régua olha o bloco que o pedido leva, e não o que a escolha achou: o
//! exemplo que o teto do bloco cortou não conta, e os que ficaram são
//! conferidos de novo contra o mapa — se importam contra uma regra forte, se
//! ainda existem com o nome e as linhas que o pedido cita —, assim como cada
//! arquivo que a receita do git cita e cada regra, que só vale se a tarefa
//! toca um dos dois papéis dela. Ao lado, a régua conta quantos exemplos a
//! mesma escolha daria sem o filtro das regras e quantos deles furariam uma.

use std::time::{Duration, Instant};

use super::*;
use crate::domain::project_map::RecipeOf;
use crate::domain::spec_events::SpecEvent;
use crate::domain::wave_prompt::{
    estimate_tokens, pattern_block, pattern_block_in_step, recipe_lines, PATTERN_CAP, STEP_INDENT,
};
use crate::platform::i18n::translate;

/// O que a régua achou num bloco do padrão.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Ruler {
    /// As regras do bloco cujos dois papéis a tarefa não toca.
    pub foreign_rules: Vec<String>,
    /// Os exemplos que o bloco leva, depois do teto dele.
    pub examples_shown: usize,
    /// Dos exemplos do bloco, os que importam contra uma regra forte, com
    /// quantas importações vão contra.
    pub examples_against: Vec<(String, usize)>,
    /// Dos exemplos do bloco, os que o mapa já não tem, ou que já não têm o
    /// nome e as linhas que o pedido cita.
    pub examples_stale: Vec<String>,
    /// Os arquivos que as receitas do git do bloco citam.
    pub recipe_cited: usize,
    /// Dos arquivos citados, os que o mapa e o disco já não têm.
    pub recipe_gone: Vec<String>,
    /// Os exemplos que a mesma escolha daria para os arquivos da tarefa sem
    /// o filtro das regras.
    pub unfiltered_picks: usize,
    /// Desses, os que importam contra uma regra forte.
    pub unfiltered_against: usize,
}

impl Ruler {
    /// Nada a apontar: nenhuma regra alheia, nenhum exemplo que fura regra
    /// ou sumiu do mapa, nenhum arquivo de receita perdido.
    pub fn is_clean(&self) -> bool {
        self.foreign_rules.is_empty()
            && self.examples_against.is_empty()
            && self.examples_stale.is_empty()
            && self.recipe_gone.is_empty()
    }
}

/// O bloco do padrão de uma tarefa: o que custa e o que a régua achou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TaskMeasure {
    pub wave: u64,
    pub code: String,
    /// O bloco, em caracteres e em tokens ([`estimate_tokens`]).
    pub block_chars: usize,
    pub block_tokens: u64,
    /// Dos caracteres do bloco, os das receitas do git e os dos exemplos; o
    /// resto é a abertura, a linha dos arquivos grandes e as regras.
    pub recipes_chars: usize,
    pub examples_chars: usize,
    /// As regras fortes e as informações do bloco.
    pub rules: usize,
    /// Os arquivos da tarefa que passam do corte de tamanho do projeto.
    pub large: usize,
    /// Os exemplos que a escolha achou, antes do teto do bloco
    /// ([`PATTERN_CAP`]).
    pub examples_found: usize,
    /// As receitas do git do bloco.
    pub recipes: usize,
    pub ruler: Ruler,
}

/// Uma onda medida, com e sem o padrão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WaveMeasure {
    pub wave: u64,
    /// As tarefas do pedido da onda.
    pub tasks: usize,
    /// O tamanho do pedido, com o padrão e sem ele.
    pub chars_with: usize,
    pub chars_without: usize,
    pub tokens_with: u64,
    pub tokens_without: u64,
    /// A soma dos blocos, em caracteres, pela conta de cada tarefa: tem de
    /// ser a diferença exata entre os dois pedidos.
    pub blocks_chars: usize,
    /// As skills que a conferência recusou no pedido com o padrão.
    pub bad_skills: usize,
    /// Os blocos, um por tarefa que ganhou o padrão.
    pub blocks: Vec<TaskMeasure>,
}

/// A medida de uma lista de ondas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Measure {
    pub waves: Vec<WaveMeasure>,
    /// O tempo de montar o pedido de todas as ondas, com o padrão e sem ele.
    pub with_time: Duration,
    pub without_time: Duration,
}

/// Contra o que a régua confere um bloco: as partes do mapa (`map`), o mapa
/// `read` que o padrão lê, o `pattern` aprendido dele e o idioma do pedido.
struct Ground<'g> {
    map: &'g MapParts<'g>,
    read: &'g ProjectMap,
    pattern: &'g Pattern,
    lang: Locale,
}

/// As importações de `module` que vão contra uma regra forte de `pattern`:
/// a importação de um papel que a regra manda o papel do próprio arquivo
/// importar, e nunca o contrário. Contada direto das regras e das
/// importações, sem passar pela escolha dos exemplos.
fn imports_against(pattern: &Pattern, module: &MapModule) -> usize {
    let Some(own) = pattern.roles.get(&module.path) else {
        return 0;
    };
    module
        .deps
        .iter()
        .filter_map(|dep| pattern.roles.get(dep))
        .filter(|role| *role != own && pattern.strong.iter().any(|d| d.from == **role && d.to == *own))
        .count()
}

/// A linha do exemplo `e` como o bloco a escreve, sem o recuo.
fn example_line(e: &PatternExample, lang: Locale) -> String {
    translate("prompt.pattern.example", lang)
        .replace("{name}", &e.name)
        .replace("{path}", &e.path)
        .replace("{start}", &e.start.to_string())
        .replace("{end}", &e.end.to_string())
}

/// O que as receitas do git e os exemplos pesam, em caracteres, no bloco
/// `text` que `pattern_block` montou de `block`: só as peças que o teto
/// deixou, cada uma como o pedido a escreve sob o passo da tarefa, com o
/// recuo a mais de cada linha.
fn pieces_chars(block: &TaskPattern, text: &str, lang: Locale) -> (usize, usize) {
    let indent = STEP_INDENT.saturating_sub(2);
    let recipes = block
        .recipes
        .iter()
        .map(|recipe| recipe_lines(recipe, lang))
        .filter(|lines| text.contains(lines.as_str()))
        .map(|lines| lines.chars().count() + lines.lines().count() * indent)
        .sum();
    let examples = block
        .examples
        .iter()
        .map(|e| format!("    - {}\n", example_line(e, lang)))
        .filter(|line| text.contains(line.as_str()))
        .map(|line| line.chars().count() + indent)
        .sum();
    (recipes, examples)
}

/// Os arquivos que uma receita cita: o que ela muda e o que mudou junto.
fn cited_by(recipe: &Recipe) -> Vec<&str> {
    let own = match &recipe.of {
        RecipeOf::Changed(path) => Some(path.as_str()),
        RecipeOf::Created(_) => None,
    };
    own.into_iter()
        .chain(recipe.together.iter().map(|(path, _)| path.as_str()))
        .collect()
}

/// O arquivo `path` existe no mapa atual — como arquivo dele ou como caminho
/// da história do git — e no disco de `root`.
fn still_there(root: &Path, read: &ProjectMap, path: &str) -> bool {
    let mapped = read.module(path).is_some() || read.history.paths.iter().any(|known| known == path);
    mapped && root.join(path).exists()
}

/// A régua sobre o bloco `pattern` de uma tarefa que mexe em `files`.
/// `unfiltered` guarda, por arquivo, os exemplos que a escolha daria sem o
/// filtro das regras e quantos deles furam uma: cada arquivo se conta uma vez
/// só por medida.
fn ruler(
    ground: &Ground<'_>,
    files: &[String],
    block: &TaskPattern,
    unfiltered: &mut BTreeMap<String, (usize, usize)>,
) -> Ruler {
    let Ground {
        map,
        read,
        pattern,
        lang,
    } = *ground;
    let mut out = Ruler::default();
    let roles: BTreeSet<&str> = files
        .iter()
        .filter_map(|path| pattern.roles.get(path))
        .map(String::as_str)
        .collect();
    for d in block.strong.iter().chain(&block.info) {
        if !roles.contains(d.from.as_str()) && !roles.contains(d.to.as_str()) {
            out.foreign_rules.push(format!("{} -> {}", d.from, d.to));
        }
    }
    let text = pattern_block(block, lang);
    for e in block.examples.iter().filter(|e| text.contains(&example_line(e, lang))) {
        out.examples_shown += 1;
        let declared = map.declarations(&e.path, &e.name).is_some_and(|named| {
            named
                .modules
                .iter()
                .filter(|m| m.path == e.path)
                .flat_map(|m| &m.declarations)
                .any(|d| d.name == e.name && d.line == e.start && d.end_line == e.end)
        });
        match read.module(&e.path) {
            Some(module) if declared => {
                let against = imports_against(pattern, module);
                if against > 0 {
                    out.examples_against.push((e.path.clone(), against));
                }
            }
            _ => out.examples_stale.push(e.path.clone()),
        }
    }
    for path in block.recipes.iter().flat_map(cited_by) {
        out.recipe_cited += 1;
        if !still_there(map.root, read, path) {
            out.recipe_gone.push(path.to_string());
        }
    }
    if !block.strong.is_empty() || !block.info.is_empty() {
        for file in files {
            let (picks, against) = *unfiltered.entry(file.clone()).or_insert_with(|| {
                let picks = examples_following(read, file, lang, &Pattern::default()).picks;
                let against = picks
                    .iter()
                    .filter_map(|p| read.module(&p.path))
                    .filter(|m| imports_against(pattern, m) > 0)
                    .count();
                (picks.len(), against)
            });
            out.unfiltered_picks += picks;
            out.unfiltered_against += against;
        }
    }
    out
}

/// Os arquivos que a tarefa `task` cita.
fn files_of(task: &SpecEvent) -> Vec<String> {
    let files = task
        .fields
        .get("files")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    files
        .iter()
        .filter_map(|file| file.get("path").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// As ondas de `log` que já têm entrega gravada, em ordem de número.
pub(super) fn delivered_waves(log: &SpecLog) -> Vec<u64> {
    let mut waves: Vec<u64> = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "delivered")
        .filter_map(SpecEvent::wave)
        .collect();
    waves.sort_unstable();
    waves.dedup();
    waves.retain(|wave| log.planned_waves().contains(wave));
    waves
}

/// O pedido de cada onda de `waves`, com o padrão e sem ele, e a régua sobre
/// o bloco de cada tarefa. O mapa é lido por `read`, e a história além da
/// janela dele nunca é lida do git.
pub(super) fn measure(
    root: &Path,
    spec: &str,
    log: &SpecLog,
    waves: &[u64],
    lang: Locale,
    read: &MapReader<'_>,
) -> Measure {
    let trace = |_: &str, _: usize| false;
    let blind = |need: Need<'_>| -> Result<ProjectMap, MapRefusal> {
        match need {
            Need::Examples { .. } => Err(MapRefusal::MapMissing),
            other => read(other),
        }
    };
    let assemble = |reader: &MapReader<'_>| {
        let started = Instant::now();
        let built = prompts_reading(root, spec, log, lang, &Flight::default(), reader, &trace);
        (built, started.elapsed())
    };
    // A primeira montagem aquece o disco; só as duas seguintes contam o tempo.
    let _ = assemble(read);
    let (with, with_time) = assemble(read);
    let (without, without_time) = assemble(&blind);

    let languages = Languages::of_project(root);
    let codes = log.codes();
    let map = MapParts::new(root, read, &trace);
    let mut unfiltered: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut measured = Vec::new();
    for wave in waves.iter().copied() {
        let (Some(seen), Some(unseen)) = (
            with.iter().find(|p| p.wave == wave),
            without.iter().find(|p| p.wave == wave),
        ) else {
            continue;
        };
        let items = request_items(log, None, wave, None, &languages).items;
        let tasks: Vec<&SpecEvent> = items.into_iter().filter(|e| e.event_type == "task").collect();
        let patterns = task_patterns(&map, log, &tasks, &codes, lang);
        let mut blocks = Vec::new();
        if let (Some(read_map), Some(learned)) = (map.pattern(), map.learned(Vec::new)) {
            let ground = Ground {
                map: &map,
                read: read_map,
                pattern: learned,
                lang,
            };
            for task in &tasks {
                let code = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
                let Some(block) = patterns.get(&code) else { continue };
                let text = pattern_block(block, lang);
                let sent = pattern_block_in_step(block, lang);
                let (recipes_chars, examples_chars) = pieces_chars(block, &text, lang);
                blocks.push(TaskMeasure {
                    wave,
                    code,
                    block_chars: sent.chars().count(),
                    block_tokens: estimate_tokens(&sent),
                    recipes_chars,
                    examples_chars,
                    rules: block.strong.len() + block.info.len(),
                    large: block.large.len(),
                    examples_found: block.examples.len(),
                    recipes: block.recipes.len(),
                    ruler: ruler(&ground, &files_of(task), block, &mut unfiltered),
                });
            }
        }
        measured.push(WaveMeasure {
            wave,
            tasks: tasks.len(),
            chars_with: seen.text.chars().count(),
            chars_without: unseen.text.chars().count(),
            tokens_with: estimate_tokens(&seen.text),
            tokens_without: estimate_tokens(&unseen.text),
            blocks_chars: blocks.iter().map(|b| b.block_chars).sum(),
            bad_skills: seen.bad_skills.len(),
            blocks,
        });
    }
    Measure {
        waves: measured,
        with_time,
        without_time,
    }
}

/// O texto do relatório da medida: o custo em tokens do padrão e o que a
/// régua achou, em números.
pub(super) fn report(measure: &Measure) -> String {
    let waves = &measure.waves;
    let blocks: Vec<&TaskMeasure> = waves.iter().flat_map(|w| &w.blocks).collect();
    let tasks: usize = waves.iter().map(|w| w.tasks).sum();
    let with: u64 = waves.iter().map(|w| w.tokens_with).sum();
    let without: u64 = waves.iter().map(|w| w.tokens_without).sum();
    let extra = with - without;
    let percent = |part: u64, whole: u64| {
        if whole == 0 {
            0.0
        } else {
            part as f64 * 100.0 / whole as f64
        }
    };
    let mut sizes: Vec<u64> = blocks.iter().map(|b| b.block_tokens).collect();
    sizes.sort_unstable();
    let at = |share: usize| {
        sizes
            .get(((sizes.len() * share).div_ceil(100)).saturating_sub(1))
            .copied()
            .unwrap_or(0)
    };
    let mean = if sizes.is_empty() {
        0.0
    } else {
        sizes.iter().sum::<u64>() as f64 / sizes.len() as f64
    };
    let sum = |pick: fn(&Ruler) -> usize| blocks.iter().map(|b| pick(&b.ruler)).sum::<usize>();
    let mut out = String::new();
    let mut line = |text: String| {
        out.push_str(&text);
        out.push('\n');
    };
    line(format!(
        "ondas medidas: {}; tarefas: {}; tarefas com bloco do padrão: {}",
        waves.len(),
        tasks,
        blocks.len()
    ));
    line(format!(
        "tokens do pedido, soma das ondas: sem o padrão {without}; com {with}; a mais {extra} ({:.1}% do pedido sem)",
        percent(extra, without)
    ));
    line(format!(
        "bloco por tarefa, em tokens: média {mean:.1}; mediana {}; p90 {}; maior {}",
        at(50),
        at(90),
        sizes.last().copied().unwrap_or(0)
    ));
    let chars: usize = blocks.iter().map(|b| b.block_chars).sum();
    let share = |part: usize| percent(part as u64, chars as u64);
    let (recipes, examples) = (
        blocks.iter().map(|b| b.recipes_chars).sum::<usize>(),
        blocks.iter().map(|b| b.examples_chars).sum::<usize>(),
    );
    line(format!(
        "de que o bloco é feito, em caracteres: abertura, regras e arquivo grande {:.1}%; receita do git {:.1}%; exemplos {:.1}%",
        share(chars - recipes - examples),
        share(recipes),
        share(examples)
    ));
    line(format!(
        "tarefas com bloco que levam: regra {}; arquivo grande {}; receita {}; exemplo {}",
        blocks.iter().filter(|b| b.rules > 0).count(),
        blocks.iter().filter(|b| b.large > 0).count(),
        blocks.iter().filter(|b| b.recipes > 0).count(),
        blocks.iter().filter(|b| b.ruler.examples_shown > 0).count()
    ));
    line(format!(
        "exemplos que o teto de {PATTERN_CAP} caracteres do bloco cortou: {} de {}; ondas em que a diferença dos pedidos não é a soma dos blocos: {}",
        blocks.iter().map(|b| b.examples_found - b.ruler.examples_shown).sum::<usize>(),
        blocks.iter().map(|b| b.examples_found).sum::<usize>(),
        waves.iter().filter(|w| w.chars_with - w.chars_without != w.blocks_chars).count()
    ));
    line(format!(
        "tempo de montar o pedido de todas as ondas: sem {} ms; com {} ms",
        measure.without_time.as_millis(),
        measure.with_time.as_millis()
    ));
    line(format!(
        "régua, exemplos: {} no pedido; {} contra uma regra; {} que o mapa já não tem; sem o filtro das regras a escolha daria {} e {} furariam uma",
        sum(|r| r.examples_shown),
        sum(|r| r.examples_against.len()),
        sum(|r| r.examples_stale.len()),
        sum(|r| r.unfiltered_picks),
        sum(|r| r.unfiltered_against)
    ));
    line(format!(
        "régua, receita: {} arquivos citados; {} que o mapa ou o disco já não têm; regras de papel que a tarefa não toca: {}",
        sum(|r| r.recipe_cited),
        sum(|r| r.recipe_gone.len()),
        sum(|r| r.foreign_rules.len())
    ));
    line(format!(
        "skills recusadas nos pedidos com o padrão: {}",
        waves.iter().map(|w| w.bad_skills).sum::<usize>()
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::super::tests::{log_of, project_with_a_pattern, task_lines, write_map};
    use super::*;
    use crate::domain::pattern::Direction;
    use serde_json::json;

    /// A medida da onda 1 do projeto `root`, com o mapa lido do próprio
    /// projeto.
    fn measured(root: &Path, log: &SpecLog) -> Measure {
        let read = |need: Need<'_>| crate::io::project_map::read_for(root, need);
        measure(root, "teste", log, &[1], Locale::PtBr, &read)
    }

    fn direction(from: &str, to: &str, along: usize, against: usize) -> Direction {
        Direction {
            from: from.into(),
            to: to.into(),
            along,
            against,
        }
    }

    /// As linhas do bloco do padrão sob a tarefa `title` no texto do pedido,
    /// da abertura dele até a última linha da tarefa, em caracteres, com a
    /// quebra de linha de cada uma.
    fn block_chars_in(text: &str, title: &str) -> usize {
        let head = translate("prompt.pattern.head", Locale::PtBr);
        let lines = task_lines(text, title);
        let from = lines
            .iter()
            .position(|line| line.contains(head))
            .unwrap_or_else(|| panic!("{title} sem o bloco: {text}"));
        lines[from..].iter().map(|line| line.chars().count() + 1).sum()
    }

    #[test]
    fn the_request_without_the_pattern_is_the_one_with_it_minus_the_blocks() {
        let (dir, log) = project_with_a_pattern();
        let root = dir.path();
        let wave = &measured(root, &log).waves[0];
        // O pedido "com" da medida é o que a rodada monta: o bloco de cada
        // tarefa, lido do texto dele, é o que separa os dois pedidos.
        let sent = &prompts(root, "teste", &log, Locale::PtBr, &Flight::default())[0].text;
        assert_eq!(wave.chars_with, sent.chars().count());
        let in_request = block_chars_in(sent, "Criar o controller") + block_chars_in(sent, "Criar o service");
        assert_eq!(
            in_request, 940,
            "a regra e os três exemplos de cada tarefa, 488 e 452 caracteres, com o recuo do passo"
        );
        assert_eq!(wave.blocks_chars, in_request);
        assert_eq!(wave.chars_with - wave.chars_without, in_request);
        // Em tokens: 587 com o padrão, 352 sem ele, 235 a mais; os dois
        // blocos somam 122 e 113, e cada um arredonda para cima à parte.
        assert_eq!((wave.tokens_with, wave.tokens_without), (587, 352));
        let blocks: Vec<u64> = wave.blocks.iter().map(|b| b.block_tokens).collect();
        assert_eq!(blocks, [122, 113]);
        assert!((wave.tokens_with - wave.tokens_without).abs_diff(blocks.iter().sum()) <= 1);
        let shape: Vec<(usize, usize, usize)> = wave
            .blocks
            .iter()
            .map(|b| (b.rules, b.examples_found, b.recipes))
            .collect();
        assert_eq!(
            shape,
            [(1, 3, 0), (1, 3, 0)],
            "uma regra, três exemplos e nenhuma receita por tarefa"
        );
        // O peso dos exemplos vem da linha de cada um no bloco: sem receita,
        // o resto é a abertura e a regra.
        for block in &wave.blocks {
            let lines = |text: &str| text.lines().filter(|l| l.contains("exemplo:")).map(|l| l.chars().count() + 1).sum::<usize>();
            let text = task_lines(sent, if block.code == wave.blocks[0].code { "Criar o controller" } else { "Criar o service" }).join("\n");
            assert_eq!(block.recipes_chars, 0, "{block:?}");
            assert_eq!(block.examples_chars, lines(&text), "{block:?}: {text}");
            assert!(block.examples_chars > 0 && block.examples_chars < block.block_chars, "{block:?}");
        }
    }

    #[test]
    fn a_project_without_a_pattern_costs_nothing_and_the_two_requests_are_equal() {
        let (dir, log) = project_with_a_pattern();
        let root = dir.path();
        // Sem regra de importação nenhuma, o padrão não escreve bloco.
        write_map(
            root,
            &json!({"modules": [{"path": "src/a.rs", "language": "rust", "loc": 10}]}),
        );
        let wave = &measured(root, &log).waves[0];
        assert!(wave.blocks.is_empty());
        assert_eq!(
            (wave.chars_with, wave.tokens_with),
            (wave.chars_without, wave.tokens_without)
        );
    }

    #[test]
    fn the_ruler_passes_what_the_request_carries_and_counts_what_the_filter_kept_out() {
        let (dir, log) = project_with_a_pattern();
        let wave = &measured(dir.path(), &log).waves[0];
        let by_task: Vec<&Ruler> = wave.blocks.iter().map(|b| &b.ruler).collect();
        assert!(by_task.iter().all(|ruler| ruler.is_clean()), "{by_task:?}");
        assert_eq!(by_task.iter().map(|r| r.examples_shown).collect::<Vec<_>>(), [3, 3]);
        // Sem o filtro das regras, o service que importa um controller — o
        // mais recente da pasta, e testado — entra entre os três exemplos do
        // service novo, e nos do controller não há nenhum que fure.
        assert_eq!(
            by_task
                .iter()
                .map(|r| (r.unfiltered_picks, r.unfiltered_against))
                .collect::<Vec<_>>(),
            [(3, 0), (3, 1)]
        );
    }

    #[test]
    fn the_ruler_flags_an_example_against_a_rule_a_lost_recipe_file_and_a_foreign_rule() {
        let (dir, _) = project_with_a_pattern();
        let root = dir.path();
        let read = |need: Need<'_>| crate::io::project_map::read_for(root, need);
        let trace = |_: &str, _: usize| false;
        let map = MapParts::new(root, &read, &trace);
        let (read_map, pattern) = (map.pattern().expect("o mapa"), map.learned(Vec::new).expect("o padrão"));
        let ground = Ground {
            map: &map,
            read: read_map,
            pattern,
            lang: Locale::PtBr,
        };
        let touched = vec!["src/controller/controller0.controller.ts".to_string()];
        let example = |name: &str, path: &str, start: u64, end: u64| PatternExample {
            name: name.into(),
            path: path.into(),
            start,
            end,
        };
        let block = TaskPattern {
            strong: vec![
                direction("controller", "service", 24, 1),
                direction("repository", "entity", 30, 0),
            ],
            examples: vec![
                example("Controller1", "src/controller/controller1.controller.ts", 3, 9),
                // O service que importa um controller vai contra a regra.
                example("Service4", "src/service/service4.service.ts", 2, 8),
                // O mapa não tem o arquivo, e tem o arquivo com outras linhas.
                example("Ghost", "src/controller/ghost.controller.ts", 1, 2),
                example("Service0", "src/service/service0.service.ts", 20, 30),
            ],
            recipes: vec![Recipe {
                of: RecipeOf::Changed("src/controller/controller0.controller.ts".into()),
                commits: 3,
                together: vec![("src/lost.ts".into(), 3)],
                tests: None,
            }],
            ..TaskPattern::default()
        };
        let found = ruler(&ground, &touched, &block, &mut BTreeMap::new());
        assert_eq!(found.foreign_rules, ["repository -> entity"]);
        assert_eq!(found.examples_shown, 4);
        assert_eq!(
            found.examples_against,
            [("src/service/service4.service.ts".to_string(), 1)]
        );
        assert_eq!(
            found.examples_stale,
            ["src/controller/ghost.controller.ts", "src/service/service0.service.ts"]
        );
        assert_eq!(found.recipe_cited, 2);
        assert_eq!(found.recipe_gone, ["src/lost.ts"]);
        assert!(!found.is_clean());
    }

    /// O que as receitas e os exemplos pesam é o que o bloco leva depois do
    /// teto: com o primeiro exemplo no lugar da receita, o peso da receita é
    /// zero e o dos exemplos é a linha do primeiro; sem corte, é o de todas
    /// as peças.
    #[test]
    fn the_pieces_weigh_what_the_cap_left_in_the_block_even_when_the_first_example_replaced_the_recipe() {
        let recipe = Recipe {
            of: RecipeOf::Created("apps/rt/src/commands/area_0/*.rs".into()),
            commits: 10,
            together: vec![("apps/rt/src/commands/area_0/mod.rs".into(), 9)],
            tests: Some(7),
        };
        let examples: Vec<PatternExample> = (0..3)
            .map(|n| PatternExample {
                name: format!("create{n}"),
                path: format!("src/order{n}/order{n}.controller.ts"),
                start: 10,
                end: 40,
            })
            .collect();
        // O recuo do passo soma um espaço a cada linha do bloco.
        let line = |e: &PatternExample| format!("    - {}\n", example_line(e, Locale::PtBr)).chars().count() + 1;
        let recipe_size = recipe_lines(&recipe, Locale::PtBr).chars().count() + recipe_lines(&recipe, Locale::PtBr).lines().count();
        let strong: Vec<Direction> = (0..6)
            .map(|n| direction(&format!("controller_of_area_{n:02}"), &format!("service_of_area_{n:02}"), 40, 1))
            .collect();

        let cut = TaskPattern {
            strong: strong.clone(),
            recipes: vec![recipe.clone()],
            examples: examples.clone(),
            ..TaskPattern::default()
        };
        let text = pattern_block(&cut, Locale::PtBr);
        assert!(text.contains("`create0`") && !text.contains("Receita do git"), "{text}");
        assert_eq!(pieces_chars(&cut, &text, Locale::PtBr), (0, line(&examples[0])));

        let whole = TaskPattern {
            strong: vec![direction("controller", "service", 24, 1)],
            recipes: vec![recipe],
            examples: examples.clone(),
            ..TaskPattern::default()
        };
        let text = pattern_block(&whole, Locale::PtBr);
        assert_eq!(
            pieces_chars(&whole, &text, Locale::PtBr),
            (recipe_size, examples.iter().map(line).sum::<usize>()),
            "{text}"
        );
    }

    #[test]
    fn the_examples_the_cap_cut_are_not_checked_and_do_not_count_as_shown() {
        let (dir, _) = project_with_a_pattern();
        let root = dir.path();
        let read = |need: Need<'_>| crate::io::project_map::read_for(root, need);
        let trace = |_: &str, _: usize| false;
        let map = MapParts::new(root, &read, &trace);
        let (read_map, pattern) = (map.pattern().expect("o mapa"), map.learned(Vec::new).expect("o padrão"));
        let ground = Ground {
            map: &map,
            read: read_map,
            pattern,
            lang: Locale::PtBr,
        };
        // Vinte exemplos com nome longo passam do teto do bloco: só os
        // primeiros ficam, e o de baixo, que iria contra a regra, sai.
        let mut examples: Vec<PatternExample> = (0..20)
            .map(|n| PatternExample {
                name: format!("Controller{}", n % 5),
                path: format!("src/controller/controller{}.controller.ts", n % 5),
                start: 3,
                end: 9,
            })
            .collect();
        examples.push(PatternExample {
            name: "Service4".into(),
            path: "src/service/service4.service.ts".into(),
            start: 2,
            end: 8,
        });
        let block = TaskPattern {
            strong: vec![direction("controller", "service", 24, 1)],
            examples,
            ..TaskPattern::default()
        };
        let found = ruler(
            &ground,
            &["src/controller/controller0.controller.ts".to_string()],
            &block,
            &mut BTreeMap::new(),
        );
        assert!(
            found.examples_shown > 0 && found.examples_shown < 21,
            "{}",
            found.examples_shown
        );
        assert!(found.examples_against.is_empty(), "{found:?}");
    }

    #[test]
    fn a_wave_counts_as_delivered_only_with_a_delivery_and_still_in_the_plan() {
        let log = log_of(&[
            (
                "wave",
                json!({"n": 1, "text": "A", "criteria": [], "done_when": "passa"}),
            ),
            (
                "task",
                json!({"wave": 1, "text": "Um", "files": [{"path": "src/a.rs"}]}),
            ),
            (
                "wave",
                json!({"n": 2, "text": "B", "criteria": [], "done_when": "passa"}),
            ),
            (
                "task",
                json!({"wave": 2, "text": "Dois", "files": [{"path": "src/b.rs"}]}),
            ),
            (
                "delivered",
                json!({"wave": 1, "text": "feito", "files": ["src/a.rs"], "agreed": []}),
            ),
            (
                "delivered",
                json!({"wave": 7, "text": "de uma onda que saiu do plano", "files": [], "agreed": []}),
            ),
        ]);
        assert_eq!(delivered_waves(&log), [1]);
    }

    /// A medida do padrão sobre as tarefas já entregues da spec real e o
    /// mapa do projeto, que ficam fora do git: imprime o relatório e falha se
    /// a régua achar algo. Roda à mão (`--ignored --nocapture`) com
    /// `MUSTARD_SPEC_FILE` (o arquivo de eventos da spec) e
    /// `MUSTARD_MEASURE_MAP` (uma cópia do mapa do projeto); o projeto é a
    /// pasta do repositório, ou `MUSTARD_MEASURE_ROOT`.
    #[test]
    #[ignore = "lê a spec real e o mapa do projeto, que ficam fora do git"]
    fn the_pattern_measured_over_the_delivered_tasks_of_the_real_spec() {
        let file = PathBuf::from(std::env::var_os("MUSTARD_SPEC_FILE").expect("MUSTARD_SPEC_FILE"));
        let model = PathBuf::from(std::env::var_os("MUSTARD_MEASURE_MAP").expect("MUSTARD_MEASURE_MAP"));
        let root = std::env::var_os("MUSTARD_MEASURE_ROOT")
            .map_or_else(|| crate::manifest_dir::manifest_dir().join("../.."), PathBuf::from);
        let log = crate::io::spec_events::read(&file)
            .expect("a spec se lê")
            .expect("a spec existe");
        let spec = file
            .parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let read = |need: Need<'_>| crate::io::project_map::read_for_at(&model, need);
        let done = measure(&root, &spec, &log, &delivered_waves(&log), Locale::PtBr, &read);
        println!("{}", report(&done));
        for wave in &done.waves {
            assert_eq!(
                wave.chars_with - wave.chars_without,
                wave.blocks_chars,
                "onda {}",
                wave.wave
            );
            for block in &wave.blocks {
                assert!(
                    block.ruler.is_clean(),
                    "{} da onda {}: {:?}",
                    block.code,
                    wave.wave,
                    block.ruler
                );
            }
        }
    }
}
