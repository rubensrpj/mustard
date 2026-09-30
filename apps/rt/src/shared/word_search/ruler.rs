//! A régua do gasto da busca por palavra: quanto o Claude recebe e quanto ele
//! gasta, medido com as buscas reais que ele mandou.
//!
//! Cada linha da entrada (`SPEND_INPUT`, o arquivo que o preparo junta a partir
//! da régua real e das cadeias de leitura) traz a chamada de verdade — o
//! `Bash` ou o `Grep` como o Claude a mandou, já na pasta da cópia do projeto —,
//! o arquivo que ele abriu depois e, quando a conversa deixou a cadeia, o que
//! ele recebeu do `grep` e o que leu até a primeira edição. A régua roda a
//! chamada pelo mesmo caminho que a sessão usa, o despachante do gancho de
//! `PreToolUse` ([`crate::dispatch::run_event`]), contra o mapa da cópia: o que
//! ela mede é o texto que o Claude receberia, nunca uma lista interna.
//!
//! O gasto de uma busca é medido em caracteres:
//!
//! - **hoje**: a saída do `grep` que o Claude recebeu mais o que leu até a
//!   edição;
//! - **com o Mustard**: o texto do gancho no lugar do `grep`; mais a saída
//!   inteira do `grep` quando o arquivo certo não está entre os mostrados (o
//!   Claude repete a busca); mais as leituras que sobram. A leitura sai só
//!   quando a resposta traz o trecho que ela daria: a do arquivo editado, se a
//!   resposta mostra o trecho editado; na cadeia sem edição, a leitura cujo
//!   trecho a resposta mostra por inteiro. A busca que o gancho deixa passar
//!   custa o mesmo de hoje, e a nota que ele põe junto soma o tamanho dela.
//!
//! O termômetro, sem número fixo, diz em que posição do que o gancho mostra
//! está o arquivo certo e, quando ele não está, a causa: o gancho deixou
//! passar, o arquivo está fora do mapa, o mapa o tem abaixo do quinto, o mapa
//! não achou palavra que o leve a ele, ou o gancho o tem entre os cinco e a
//! busca não achou linha nele.
//!
//! As duas metades da entrada (`A` e `B`) vêm separadas por conversa: uma
//! escolhe, a outra confere. Cada projeto sai em tabelas de cada metade e do
//! total.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mustard_core::domain::model::contract::{HookInput, Trigger, Verdict};
use mustard_core::io::map_triage::Triaged;
use mustard_core::io::project_map::{self as store, Need};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{FileHits, SHOWN_FILES};

/// Uma leitura que o Claude fez entre a busca e a edição.
#[derive(Debug, Deserialize)]
struct Read {
    file: String,
    /// A primeira e a última linha que a leitura devolveu; `0` ou `-1`
    /// quando a leitura não disse de onde partia.
    first: i64,
    last: i64,
    chars: usize,
}

/// O que a conversa guardou depois da busca: a saída dela, as leituras e a
/// edição com que a cadeia terminou.
#[derive(Debug, Deserialize)]
struct Chain {
    grep_chars: usize,
    ended: String,
    edit_file: Option<String>,
    /// As linhas do trecho editado, quando a conversa o localizou.
    edit_range: Option<(u64, u64)>,
    /// A edição é o arquivo inteiro (`Write`).
    #[serde(default)]
    edit_all: bool,
    reads: Vec<Read>,
    reads_chars: usize,
}

/// Uma busca real da régua.
#[derive(Debug, Deserialize)]
struct Row {
    key: String,
    project: String,
    name: String,
    kind: String,
    tool_name: String,
    tool_input: Value,
    half: String,
    targets: Vec<String>,
    expired: bool,
    chain: Option<Chain>,
}

/// O que uma resposta do gancho mostra: os arquivos, na ordem, e os trechos
/// que ela traz com o texto, cada um com o arquivo e as linhas de começo e de
/// fim.
#[derive(Debug, Default, PartialEq, Eq)]
struct Shown {
    files: Vec<String>,
    ranges: Vec<(String, u64, u64)>,
}

/// Os arquivos e os trechos que o texto `answer` mostra. O arquivo é a linha
/// sem recuo e sem espaço (com ou sem a marca de mudado entre parênteses); a
/// linha de mapa (`` `a`, `b` ``) traz os arquivos entre crases; o trecho é a
/// entrada `começo-fim nome` que vem seguida de texto mais recuado.
fn shown_of(answer: &str) -> Shown {
    let mut shown = Shown::default();
    let mut current: Option<String> = None;
    let mut pending: Option<(String, u64, u64)> = None;
    for line in answer.lines() {
        let indented = line.starts_with(' ');
        if indented && line.starts_with("    ") {
            if let Some(range) = pending.take() {
                shown.ranges.push(range);
            }
            continue;
        }
        pending = None;
        if indented {
            let Some(path) = current.clone() else { continue };
            let head = line.trim_start().split(' ').next().unwrap_or_default();
            if let Some((from, to)) = head.split_once('-')
                && let (Ok(from), Ok(to)) = (from.parse::<u64>(), to.parse::<u64>())
            {
                pending = Some((path, from, to));
            }
            continue;
        }
        let bare = line.split(" (").next().unwrap_or(line);
        if !bare.is_empty() && !bare.contains(' ') && (bare.contains('/') || (bare.contains('.') && !bare.ends_with('.'))) {
            if !shown.files.iter().any(|seen| seen == bare) {
                shown.files.push(bare.to_string());
            }
            current = Some(bare.to_string());
            continue;
        }
        current = None;
        let mut ticks = line.split('`');
        ticks.next();
        for (at, piece) in ticks.enumerate() {
            if at % 2 == 0 && piece.contains('/') && !shown.files.iter().any(|seen| seen == piece) {
                shown.files.push(piece.to_string());
            }
        }
    }
    shown
}

/// `true` quando algum trecho de `shown` no arquivo `file` traz as linhas de
/// `from` a `to`.
fn covers(shown: &Shown, file: &str, from: u64, to: u64) -> bool {
    shown.ranges.iter().any(|(path, start, end)| path == file && *start <= from && to <= *end)
}

/// O gasto de uma busca, em caracteres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Spend {
    today: usize,
    with: usize,
    /// Quanto da leitura a resposta dispensou.
    saved_reads: usize,
}

/// O que o gancho respondeu à busca.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// A resposta no lugar da busca, e o que ela mostra.
    Answer(String),
    /// A busca roda, com esta nota junto.
    Note(String),
    /// A busca roda como veio.
    Pass,
}

/// O gasto de `row` quando o gancho responde `outcome`. `None` sem a cadeia da
/// conversa.
fn spend_of(row: &Row, outcome: &Outcome, shown: &Shown) -> Option<Spend> {
    let chain = row.chain.as_ref()?;
    let today = chain.grep_chars + chain.reads_chars;
    let text = |text: &str| text.chars().count();
    let spend = match outcome {
        Outcome::Pass => Spend { today, with: today, saved_reads: 0 },
        Outcome::Note(note) => Spend { today, with: today + text(note), saved_reads: 0 },
        Outcome::Answer(answer) => {
            let edited = chain.ended == "edicao";
            let right: Vec<&str> = match (&chain.edit_file, edited) {
                (Some(file), true) => vec![file.as_str()],
                _ => row.targets.iter().map(String::as_str).collect(),
            };
            let hit = shown.files.iter().any(|file| right.contains(&file.as_str()));
            if !hit {
                Spend { today, with: text(answer) + today, saved_reads: 0 }
            } else {
                let saved: usize = chain
                    .reads
                    .iter()
                    .filter(|read| {
                        if edited {
                            chain.edit_file.as_deref() == Some(read.file.as_str())
                                && !chain.edit_all
                                && chain.edit_range.is_some_and(|(from, to)| covers(shown, &read.file, from, to))
                        } else {
                            right.contains(&read.file.as_str())
                                && read.first > 0
                                && covers(shown, &read.file, read.first as u64, read.last.max(read.first) as u64)
                        }
                    })
                    .map(|read| read.chars)
                    .sum();
                Spend { today, with: text(answer) + today - chain.grep_chars - saved, saved_reads: saved }
            }
        }
    };
    Some(spend)
}

/// O destino de uma busca no termômetro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Fate {
    /// O gancho deixou a busca passar, com nota ou sem.
    Passed,
    /// O arquivo certo é o de posição `n` entre os mostrados.
    At(usize),
    /// O arquivo certo está fora do mapa.
    OutsideMap,
    /// O mapa tem o arquivo abaixo do quinto.
    BelowFifth,
    /// O mapa não achou palavra que leve ao arquivo.
    NoWord,
    /// O arquivo está entre os cinco do mapa, e a busca não achou linha nele.
    NoLine,
}

thread_local! {
    /// Os arquivos que a triagem da última busca deu, na ordem dela: o que o
    /// gancho usou de verdade para ordenar a resposta.
    static LAST_ORDER: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// Guarda a ordem que a triagem da busca deu, para o termômetro dizer onde
/// estava o arquivo certo.
pub(super) fn remember_order(triaged: &Triaged) {
    let order = triaged.files.iter().map(|file| file.path.clone()).collect();
    LAST_ORDER.with(|last| *last.borrow_mut() = Some(order));
}

thread_local! {
    /// Os arquivos em que a última busca achou linha, com quantas.
    static LAST_HITS: std::cell::RefCell<Vec<(String, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Guarda os arquivos em que a busca achou linha.
pub(super) fn remember_hits(hits: &[FileHits]) {
    let found = hits.iter().map(|file| (file.path.clone(), file.lines.len())).collect();
    LAST_HITS.with(|last| *last.borrow_mut() = found);
}

/// Por que a resposta não traz nenhum dos arquivos `targets`: o que o gancho
/// ordenou na última busca (com a pasta pedida, como ele a leu) e o mapa.
fn cause_of(root: &Path, row: &Row) -> Fate {
    let model = store::model_path(root);
    let Ok(paths) = store::read_for_at(&model, Need::Paths) else { return Fate::OutsideMap };
    if !paths.modules.iter().any(|module| row.targets.contains(&module.path)) {
        return Fate::OutsideMap;
    }
    let order = LAST_ORDER.with(|last| last.borrow().clone()).unwrap_or_default();
    match order.iter().position(|path| row.targets.contains(path)) {
        None => Fate::NoWord,
        Some(at) if at >= SHOWN_FILES => Fate::BelowFifth,
        Some(_) => Fate::NoLine,
    }
}

/// O destino da busca no termômetro: o gancho que a deixa passar; a posição do
/// arquivo certo entre os mostrados; ou, quando a resposta não o traz, a causa.
fn fate_of(root: &Path, row: &Row, (outcome, shown): (&Outcome, &Shown)) -> Fate {
    match outcome {
        Outcome::Pass | Outcome::Note(_) => Fate::Passed,
        Outcome::Answer(_) => match shown.files.iter().position(|file| row.targets.contains(file)) {
            Some(place) => Fate::At(place + 1),
            None => cause_of(root, row),
        },
    }
}

/// O que o despachante do gancho responde à chamada de `row`, rodada de
/// `root` numa sessão só dela, e quanto ele levou.
fn hear(root: &Path, row: &Row, session: &str) -> (Outcome, Duration) {
    let input = HookInput {
        tool_name: Some(row.tool_name.clone()),
        tool_input: row.tool_input.clone(),
        hook_event_name: Some("PreToolUse".to_string()),
        cwd: Some(root.to_string_lossy().into_owned()),
        session_id: Some(session.to_string()),
        ..HookInput::default()
    };
    LAST_ORDER.with(|last| *last.borrow_mut() = None);
    LAST_HITS.with(|last| last.borrow_mut().clear());
    let started = Instant::now();
    let verdict = crate::dispatch::run_event(Some(Trigger::PreToolUse), &input).verdict;
    let took = started.elapsed();
    let outcome = match verdict {
        Verdict::Deny { reason } => Outcome::Answer(reason),
        Verdict::Inject { context } => Outcome::Note(context),
        _ => Outcome::Pass,
    };
    (outcome, took)
}

/// A soma de um grupo de buscas.
#[derive(Debug, Default)]
struct Sum {
    searches: usize,
    with_chain: usize,
    today: usize,
    with: usize,
    saved_reads: usize,
    fates: BTreeMap<String, usize>,
    millis: Vec<u128>,
}

impl Sum {
    fn show(&self, label: &str) -> String {
        let percent = |part: usize, whole: usize| if whole == 0 { 0.0 } else { 100.0 * part as f64 / whole as f64 };
        let gain = self.today as i64 - self.with as i64;
        let mut millis = self.millis.clone();
        millis.sort_unstable();
        let at = |share: usize| millis.get((millis.len() * share / 100).min(millis.len().saturating_sub(1))).copied().unwrap_or(0);
        let answered: usize = self.fates.iter().filter(|(fate, _)| !fate.starts_with("passou")).map(|(_, n)| n).sum();
        let first = self.fates.get("1").copied().unwrap_or(0);
        let five: usize = (1..=SHOWN_FILES).map(|n| self.fates.get(&n.to_string()).copied().unwrap_or(0)).sum();
        let fates: Vec<String> = self.fates.iter().map(|(fate, n)| format!("{fate} {n}")).collect();
        format!(
            "{label}: {} buscas | gasto em {} com cadeia: hoje {} | com o Mustard {} | diferença {gain} ({:.1}%), leitura dispensada {} | responde {} ({:.1}%), certo em 1º {first}, entre 5 {five} ({:.1}% das que respondem) | destinos: {} | tempo ms: mediana {}, p95 {}, máx {}",
            self.searches,
            self.with_chain,
            self.today,
            self.with,
            percent(gain.unsigned_abs() as usize, self.today) * gain.signum() as f64,
            self.saved_reads,
            answered,
            percent(answered, self.searches),
            percent(five, answered),
            fates.join(", "),
            at(50),
            at(95),
            millis.last().copied().unwrap_or(0),
        )
    }
}

/// O nome do destino de uma busca, como a tabela o mostra.
fn fate_name(fate: Fate) -> String {
    match fate {
        Fate::Passed => "passou".to_string(),
        Fate::At(n) => n.to_string(),
        Fate::OutsideMap => "erra: fora do mapa".to_string(),
        Fate::BelowFifth => "erra: abaixo do 5º".to_string(),
        Fate::NoWord => "erra: sem palavra".to_string(),
        Fate::NoLine => "erra: sem linha".to_string(),
    }
}

/// A régua do gasto. A entrada é `SPEND_INPUT` (o arquivo de buscas do
/// preparo) e as cópias dos projetos, cada uma com o mapa dela, estão em
/// `SPEND_TREES/<nome>`; `SPEND_OUT` recebe uma linha por busca. Só as
/// buscas do `Bash` e do `Grep` entram nas contas; as vencidas (o texto já não
/// casa com o arquivo certo na cópia) ficam de fora. Rode com
/// `env -u TYPESAFE_API_KEY HOME=<pasta vazia>` e em `--release`, para o
/// tempo ser o do gancho de verdade.
#[test]
#[ignore = "mede com as cópias dos projetos de prova e as conversas reais"]
fn measure_the_spend_of_the_search() {
    let input = std::env::var("SPEND_INPUT").expect("SPEND_INPUT points to the searches file");
    let trees = PathBuf::from(std::env::var("SPEND_TREES").expect("SPEND_TREES points to the folder of the project copies"));
    let out = std::env::var("SPEND_OUT").expect("SPEND_OUT points to the file to write");
    let rows: Vec<Row> = serde_json::from_str(&std::fs::read_to_string(input).expect("the searches file reads")).expect("the searches file parses");
    let (mut skipped, mut other): (usize, usize) = (0, 0);
    let mut warmed: Vec<String> = Vec::new();
    let mut groups: BTreeMap<(String, String), Sum> = BTreeMap::new();
    let mut lines: Vec<String> = Vec::new();
    // A sessão de cada busca é só dela e só desta medida: o gancho lembra a busca repetida numa sessão e a deixaria passar.
    let run = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_millis());
    for (at, row) in rows.iter().enumerate() {
        if row.expired {
            skipped += 1;
            continue;
        }
        if !matches!(row.kind.as_str(), "bash" | "grep") {
            other += 1;
            continue;
        }
        let root = trees.join(&row.name);
        if !warmed.contains(&row.name) {
            warmed.push(row.name.clone());
            // A primeira busca de cada mapa refaz o índice nas línguas dele: fica fora do tempo.
            let _ = hear(&root, row, &format!("warm-{run}-{at}"));
        }
        let (outcome, took) = hear(&root, row, &format!("spend-{run}-{at}"));
        let shown = match &outcome {
            Outcome::Answer(text) => shown_of(text),
            _ => Shown::default(),
        };
        let fate = fate_of(&root, row, (&outcome, &shown));
        let spend = spend_of(row, &outcome, &shown);
        for half in [row.half.as_str(), "total"] {
            let sum = groups.entry((row.project.clone(), half.to_string())).or_default();
            sum.searches += 1;
            sum.millis.push(took.as_millis());
            *sum.fates.entry(fate_name(fate)).or_default() += 1;
            if let Some(spend) = spend {
                sum.with_chain += 1;
                sum.today += spend.today;
                sum.with += spend.with;
                sum.saved_reads += spend.saved_reads;
            }
        }
        let kind = match &outcome {
            Outcome::Answer(_) => "answer",
            Outcome::Note(_) => "note",
            Outcome::Pass => "pass",
        };
        let text = match &outcome {
            Outcome::Answer(text) | Outcome::Note(text) => text.as_str(),
            Outcome::Pass => "",
        };
        lines.push(
            json!({
                "key": row.key, "project": row.project, "half": row.half, "outcome": kind, "millis": took.as_millis(),
                "fate": fate_name(fate), "shown": shown.files, "ranges": shown.ranges, "chars": text.chars().count(),
                "today": spend.map(|s| s.today), "with": spend.map(|s| s.with), "saved_reads": spend.map(|s| s.saved_reads),
                "chain": row.chain.as_ref().map(|c| c.ended.as_str()), "text": text,
                "hit_files": LAST_HITS.with(|last| last.borrow().len()),
                "target_hits": LAST_HITS.with(|last| last.borrow().iter().filter(|(path, _)| row.targets.contains(path)).cloned().collect::<Vec<_>>()),
                "target_order": LAST_ORDER.with(|last| last.borrow().as_ref().and_then(|order| order.iter().position(|path| row.targets.contains(path)))),
                "order_len": LAST_ORDER.with(|last| last.borrow().as_ref().map(Vec::len)),
            })
            .to_string(),
        );
    }
    std::fs::write(out, lines.join("\n")).expect("the rows file writes");
    eprintln!("GASTO {skipped} buscas vencidas fora, {other} sem busca de texto (glob, explore, mustard) fora");
    for ((project, half), sum) in &groups {
        eprintln!("GASTO {}", sum.show(&format!("{project} {half}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::word_search::fixture;

    /// A resposta de hoje: os arquivos saem da linha sem recuo, e uma entrada
    /// sem texto embaixo não é trecho.
    #[test]
    fn a_function_list_shows_its_files_and_no_snippet() {
        let answer = "Cravado. O mapa achou \"a\" pelo nome. Esta resposta vale no lugar da busca comum.\nCada função vem com o começo e o fim, e as linhas achadas entre parênteses:\nsrc/frete.rs\n  2-6 calcular_frete (2)\nsrc/pedido.rs (mudado nesta onda)\n  1-4 fechar_pedido (2)\nFora do corte, lugares: 3, arquivos: 1. Repita a busca para ver a lista inteira.\nSe este não for o lugar, use suas ferramentas padrões: `Grep`, `Glob` e `Read`.";
        let shown = shown_of(answer);
        assert_eq!(shown.files, vec!["src/frete.rs", "src/pedido.rs"]);
        assert!(shown.ranges.is_empty(), "{shown:?}");
    }

    /// A entrada com texto mais recuado embaixo é um trecho mostrado, do
    /// começo ao fim que ela diz.
    #[test]
    fn an_entry_followed_by_its_text_is_a_shown_snippet() {
        let answer = "Cravado.\nsrc/frete.rs\n  2-6 calcular_frete (2)\n    2 | pub fn calcular_frete() {\n    3 | }\n  8-10 desconto_frete\nsrc/pedido.rs\n  1-4 fechar_pedido (2)";
        let shown = shown_of(answer);
        assert_eq!(shown.ranges, vec![("src/frete.rs".to_string(), 2, 6)]);
        assert!(covers(&shown, "src/frete.rs", 3, 5));
        assert!(!covers(&shown, "src/frete.rs", 3, 7));
        assert!(!covers(&shown, "src/pedido.rs", 1, 4));
    }

    /// A resposta só do mapa lista os arquivos entre crases.
    #[test]
    fn a_map_only_answer_lists_its_files_between_ticks() {
        let answer = "Parcial.\nA busca comum não acharia nenhuma linha com esse texto. O mapa aponta estes arquivos: `src/a.rs`, `docs/b.md`.";
        assert_eq!(shown_of(answer).files, vec!["src/a.rs", "docs/b.md"]);
    }

    fn row(chain: Chain, targets: &[&str]) -> Row {
        Row {
            key: "p|bash|1".to_string(),
            project: "p".to_string(),
            name: "p".to_string(),
            kind: "bash".to_string(),
            tool_name: "Bash".to_string(),
            tool_input: json!({}),
            half: "A".to_string(),
            targets: targets.iter().map(|t| (*t).to_string()).collect(),
            expired: false,
            chain: Some(chain),
        }
    }

    fn chain_edited() -> Chain {
        Chain {
            grep_chars: 1000,
            ended: "edicao".to_string(),
            edit_file: Some("src/a.rs".to_string()),
            edit_range: Some((10, 20)),
            edit_all: false,
            reads: vec![
                Read { file: "src/a.rs".to_string(), first: 1, last: 100, chars: 4000 },
                Read { file: "src/b.rs".to_string(), first: 1, last: 50, chars: 2000 },
            ],
            reads_chars: 6000,
        }
    }

    /// A busca que o gancho deixa passar custa o mesmo de hoje; a nota soma
    /// o tamanho dela.
    #[test]
    fn a_search_the_hook_passes_costs_what_it_costs_today() {
        let row = row(chain_edited(), &["src/a.rs"]);
        let spend = spend_of(&row, &Outcome::Pass, &Shown::default()).unwrap();
        assert_eq!((spend.today, spend.with), (7000, 7000));
        let noted = spend_of(&row, &Outcome::Note("abc".to_string()), &Shown::default()).unwrap();
        assert_eq!((noted.today, noted.with), (7000, 7003));
    }

    /// A resposta que traz o arquivo editado troca a saída do `grep` pelo texto
    /// dela e deixa as leituras; a que não o traz soma a saída inteira do
    /// `grep` de novo.
    #[test]
    fn an_answer_replaces_the_grep_output_and_a_miss_repeats_it() {
        let row = row(chain_edited(), &["src/a.rs"]);
        let answer = "x".repeat(300);
        let shown = Shown { files: vec!["src/a.rs".to_string()], ranges: vec![] };
        let hit = spend_of(&row, &Outcome::Answer(answer.clone()), &shown).unwrap();
        assert_eq!((hit.today, hit.with, hit.saved_reads), (7000, 6300, 0));
        let other = Shown { files: vec!["src/c.rs".to_string()], ranges: vec![] };
        let miss = spend_of(&row, &Outcome::Answer(answer), &other).unwrap();
        assert_eq!((miss.today, miss.with), (7000, 7300));
    }

    /// A leitura do arquivo editado sai só quando o trecho que a resposta traz
    /// cobre o trecho editado; as de outro arquivo ficam.
    #[test]
    fn the_read_of_the_edited_file_goes_only_when_the_answer_shows_the_edited_lines() {
        let row = row(chain_edited(), &["src/a.rs"]);
        let answer = "y".repeat(500);
        let covering = Shown { files: vec!["src/a.rs".to_string()], ranges: vec![("src/a.rs".to_string(), 5, 30)] };
        let spend = spend_of(&row, &Outcome::Answer(answer.clone()), &covering).unwrap();
        assert_eq!((spend.with, spend.saved_reads), (500 + 7000 - 1000 - 4000, 4000));
        let short = Shown { files: vec!["src/a.rs".to_string()], ranges: vec![("src/a.rs".to_string(), 5, 15)] };
        let spend = spend_of(&row, &Outcome::Answer(answer), &short).unwrap();
        assert_eq!((spend.with, spend.saved_reads), (500 + 7000 - 1000, 0));
    }

    /// Na cadeia sem edição, sai a leitura do arquivo certo que o trecho da
    /// resposta traz por inteiro.
    #[test]
    fn without_an_edit_the_read_that_the_answer_shows_whole_goes() {
        let mut chain = chain_edited();
        chain.ended = "usuario".to_string();
        chain.edit_file = None;
        chain.edit_range = None;
        chain.reads[0] = Read { file: "src/a.rs".to_string(), first: 10, last: 40, chars: 1500 };
        chain.reads_chars = 3500;
        let row = row(chain, &["src/a.rs"]);
        let shown = Shown { files: vec!["src/a.rs".to_string()], ranges: vec![("src/a.rs".to_string(), 8, 60)] };
        let spend = spend_of(&row, &Outcome::Answer("z".repeat(200)), &shown).unwrap();
        assert_eq!((spend.today, spend.with, spend.saved_reads), (4500, 200 + 4500 - 1000 - 1500, 1500));
    }

    fn bash_row(command: &str, targets: &[&str]) -> Row {
        Row { tool_input: json!({ "command": command }), ..row(chain_edited(), targets) }
    }

    /// O destino da busca de `row` depois que o gancho a ouviu de verdade: a
    /// ordem que a triagem deu vem da própria busca, e a resposta mostra os
    /// arquivos `shown`.
    fn fate_after_hearing(root: &Path, row: &Row, shown: &[&str]) -> Fate {
        let (_, _) = hear(root, row, "s-termometro");
        let shown = Shown { files: shown.iter().map(|f| (*f).to_string()).collect(), ranges: vec![] };
        fate_of(root, row, (&Outcome::Answer("resposta".to_string()), &shown))
    }

    /// A régua ouve o que o despachante do gancho responde ao mesmo texto na
    /// mesma pasta: a pasta pedida é a que o gancho procura, e a mesma palavra
    /// em outra pasta corre como veio.
    #[test]
    fn the_ruler_hears_what_the_hook_answers_to_the_same_text_in_the_same_folder() {
        let (_dir, root) = fixture::repo("{}");
        let (inside, _) = hear(&root, &bash_row("grep -rn calcular_frete src", &["src/frete.rs"]), "s-dentro");
        let Outcome::Answer(text) = inside else { panic!("an answer was expected, got {inside:?}") };
        assert!(text.contains("src/frete.rs\n  2-6 calcular_frete"), "{text}");
        assert_eq!(shown_of(&text).files.first().map(String::as_str), Some("src/frete.rs"));

        let (outside, _) = hear(&root, &bash_row("grep -rn calcular_frete docs", &[]), "s-fora");
        assert!(matches!(outside, Outcome::Pass | Outcome::Note(_)), "{outside:?}");
    }

    /// O termômetro diz a posição do arquivo certo entre os mostrados e, quando
    /// a resposta não o traz, a causa: fora do mapa, sem palavra, ou entre os
    /// cinco do mapa sem linha achada.
    #[test]
    fn the_thermometer_gives_the_place_among_the_shown_files_and_the_cause_of_each_miss() {
        let (_dir, root) = fixture::repo("{}");

        let pedido = bash_row("grep -rn fechar_pedido src", &["src/pedido.rs"]);
        assert_eq!(fate_after_hearing(&root, &pedido, &["src/frete.rs", "src/pedido.rs"]), Fate::At(2));
        assert_eq!(fate_of(&root, &pedido, (&Outcome::Pass, &Shown::default())), Fate::Passed);
        assert_eq!(fate_of(&root, &pedido, (&Outcome::Note("n".to_string()), &Shown::default())), Fate::Passed);

        let note = bash_row("grep -rn imposto docs", &["docs/notas.md"]);
        assert_eq!(fate_after_hearing(&root, &note, &["src/frete.rs"]), Fate::OutsideMap);

        let unknown = bash_row("grep -rn zzxqkw src", &["src/frete.rs"]);
        assert_eq!(fate_after_hearing(&root, &unknown, &["src/pedido.rs"]), Fate::NoWord);

        let frete = bash_row("grep -rn calcular_frete src", &["src/frete.rs"]);
        assert_eq!(fate_after_hearing(&root, &frete, &["src/pedido.rs"]), Fate::NoLine);
    }

    /// O arquivo que o mapa tem abaixo do quinto, depois de cinco que casam
    /// melhor, é a causa "abaixo do 5º".
    #[test]
    fn a_right_file_the_map_ranks_below_the_fifth_is_told_apart() {
        let mut modules = Vec::new();
        let mut files: Vec<(String, String)> = Vec::new();
        for n in 1..=6 {
            let (path, name) = (format!("src/frete{n}.rs"), format!("calcular_frete_{n}"));
            files.push((path.clone(), format!("pub fn {name}() {{}}\n")));
            modules.push(json!({ "path": path, "language": "rust", "loc": 1, "declarations": [
                { "kind": "function", "name": name, "line": 1, "end_line": 1 }] }));
        }
        files.push(("src/outro.rs".to_string(), "pub fn sem_relacao() {}\n".to_string()));
        modules.push(json!({ "path": "src/outro.rs", "language": "rust", "loc": 1, "declarations": [
            { "kind": "function", "name": "sem_relacao", "line": 1, "end_line": 1, "body_comment": "calcular frete" }] }));
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let (_dir, root) = fixture::repo_with("{}", &refs, json!({ "modules": modules }));
        let row = bash_row("grep -rn calcular_frete src", &["src/outro.rs"]);
        assert_eq!(fate_after_hearing(&root, &row, &["src/frete1.rs"]), Fate::BelowFifth);
    }
}
