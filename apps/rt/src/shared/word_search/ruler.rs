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
//!   custa o mesmo de hoje. A nota que ele põe junto da busca soma o tamanho
//!   dela e a saída inteira do `grep` fica, porque a busca roda; a leitura sai
//!   pela mesma regra da resposta, se o código da nota traz o trecho.
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
use mustard_core::io::measure_proof::{BuiltStamp, MeasureGate};
use mustard_core::io::project_map::{self as store, Need};
use mustard_core::platform::error::Result as CoreResult;
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
    /// A busca roda, com esta nota junto, que traz o código como a resposta.
    Note(String),
    /// A busca roda como veio.
    Pass,
}

/// O que o gancho mostra na sua resposta ou na nota: o texto de uma e da outra
/// tem o mesmo formato. A busca que passa sem nota não mostra nada.
fn shown_by(outcome: &Outcome) -> Shown {
    match outcome {
        Outcome::Answer(text) | Outcome::Note(text) => shown_of(text),
        Outcome::Pass => Shown::default(),
    }
}

/// Os arquivos que a busca de `row` tem de mostrar: o editado, quando a cadeia
/// terminou numa edição; senão, os que a busca real levou o Claude a abrir.
fn right_files<'a>(row: &'a Row, chain: &'a Chain) -> Vec<&'a str> {
    match (&chain.edit_file, chain.ended == "edicao") {
        (Some(file), true) => vec![file.as_str()],
        _ => row.targets.iter().map(String::as_str).collect(),
    }
}

/// Quantos caracteres de leitura o código que `shown` traz dispensa ao Claude:
/// na cadeia com edição, a leitura do arquivo editado cujo trecho cobre as
/// linhas editadas; na cadeia sem edição, a do arquivo certo cujo trecho cobre
/// as linhas lidas. A leitura de outro arquivo fica.
fn saved_reads_of(row: &Row, chain: &Chain, shown: &Shown) -> usize {
    let edited = chain.ended == "edicao";
    let right = right_files(row, chain);
    chain
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
        .sum()
}

/// O gasto de `row` quando o gancho responde `outcome`. `None` sem a cadeia da
/// conversa. A resposta no lugar da busca troca a saída do `grep` pelo texto
/// dela; a nota vai junto da busca, que roda inteira, e o Claude recebe as duas
/// saídas. Nas duas, a leitura que o código mostrado cobre sai.
fn spend_of(row: &Row, outcome: &Outcome, shown: &Shown) -> Option<Spend> {
    let chain = row.chain.as_ref()?;
    let today = chain.grep_chars + chain.reads_chars;
    let text = |text: &str| text.chars().count();
    let spend = match outcome {
        Outcome::Pass => Spend { today, with: today, saved_reads: 0 },
        Outcome::Note(note) => {
            let saved = saved_reads_of(row, chain, shown);
            Spend { today, with: today + text(note) - saved, saved_reads: saved }
        }
        Outcome::Answer(answer) => {
            let right = right_files(row, chain);
            if shown.files.iter().any(|file| right.contains(&file.as_str())) {
                let saved = saved_reads_of(row, chain, shown);
                Spend { today, with: text(answer) + today - chain.grep_chars - saved, saved_reads: saved }
            } else {
                Spend { today, with: text(answer) + today, saved_reads: 0 }
            }
        }
    };
    Some(spend)
}

/// O destino de uma busca no termômetro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Fate {
    /// O gancho deixou a busca passar, sem nota ou com uma nota que não traz o
    /// arquivo certo.
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
/// arquivo certo entre os mostrados, na resposta ou na nota; ou, quando a
/// resposta não o traz, a causa. A nota sem o arquivo certo segue como busca
/// que passou.
fn fate_of(root: &Path, row: &Row, (outcome, shown): (&Outcome, &Shown)) -> Fate {
    let place = shown.files.iter().position(|file| row.targets.contains(file));
    match (outcome, place) {
        (Outcome::Pass, _) | (Outcome::Note(_), None) => Fate::Passed,
        (Outcome::Answer(_) | Outcome::Note(_), Some(place)) => Fate::At(place + 1),
        (Outcome::Answer(_), None) => cause_of(root, row),
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

/// O carimbo que a compilação deste programa deixou nele: a versão e o resumo
/// do que estava por comitar.
fn built_stamp() -> BuiltStamp<'static> {
    BuiltStamp { version: env!("MUSTARD_VERSION_FULL"), diff: env!("MUSTARD_GIT_DIFF") }
}

/// Confere cada mapa de `maps` na porta `gate`, na ordem; o primeiro de
/// marca diferente da que o scan compilado diz recusa, e nenhum número sai.
fn check_maps(mut gate: MeasureGate, maps: &[PathBuf]) -> CoreResult<MeasureGate> {
    for map in maps {
        gate.check(map)?;
    }
    Ok(gate)
}

/// A porta comum das réguas do rt: a prova de versão deste programa e a
/// conferência dos mapas que a régua vai abrir, antes de ela medir. Sem o
/// comando de medida (`mustard-rt run measure`), com o programa compilado de
/// outro código que o da medida ou com um mapa de outra compilação do scan, a
/// régua para aqui, dizendo por quê.
pub(super) fn measure_gate(maps: &[PathBuf]) -> MeasureGate {
    MeasureGate::open(Some(&built_stamp()))
        .and_then(|gate| check_maps(gate, maps))
        .unwrap_or_else(|refusal| panic!("a régua não mede: {refusal}"))
}

/// A régua do gasto. A entrada é `SPEND_INPUT` (o arquivo de buscas do
/// preparo) e as cópias dos projetos, cada uma com o mapa dela, estão em
/// `SPEND_TREES/<nome>`; `SPEND_OUT` (ou o `--out` do comando de medida)
/// recebe uma linha por busca, cada uma com a prova de versão em `proof`. Só as
/// buscas do `Bash` e do `Grep` entram nas contas; as vencidas (o texto já não
/// casa com o arquivo certo na cópia) ficam de fora. Roda pelo comando de
/// medida, que compila o código certo em `--release`, para o tempo ser o do
/// gancho de verdade; rode com `env -u TYPESAFE_API_KEY HOME=<pasta vazia>`.
#[test]
#[ignore = "mede com as cópias dos projetos de prova e as conversas reais"]
fn measure_the_spend_of_the_search() {
    let input = std::env::var("SPEND_INPUT").expect("SPEND_INPUT points to the searches file");
    let trees = PathBuf::from(std::env::var("SPEND_TREES").expect("SPEND_TREES points to the folder of the project copies"));
    let out = mustard_core::io::measure_proof::result_path("SPEND_OUT").expect("SPEND_OUT points to the file to write");
    let rows: Vec<Row> = serde_json::from_str(&std::fs::read_to_string(input).expect("the searches file reads")).expect("the searches file parses");
    // Os mapas das cópias que entram nas contas, conferidos antes da primeira busca.
    let mut names: Vec<&str> = rows
        .iter()
        .filter(|row| !row.expired && matches!(row.kind.as_str(), "bash" | "grep"))
        .map(|row| row.name.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    let maps: Vec<PathBuf> = names.iter().map(|name| store::model_path(&trees.join(name))).collect();
    let gate = measure_gate(&maps);
    let proof = gate.proof().to_json();
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
        let shown = shown_by(&outcome);
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
                "proof": proof,
            })
            .to_string(),
        );
    }
    std::fs::write(out, lines.join("\n")).expect("the rows file writes");
    eprintln!("GASTO {skipped} buscas vencidas fora, {other} sem busca de texto (glob, explore, mustard) fora");
    for ((project, half), sum) in &groups {
        eprintln!("GASTO {}", sum.show(&format!("{project} {half}")));
    }
    for line in gate.proof().lines() {
        eprintln!("{line}");
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
        let answer = "Cravado. O mapa achou \"a\" pelo nome. Esta resposta vale no lugar da busca comum.\nCada função vem com o começo e o fim, e as linhas achadas entre parênteses:\nsrc/frete.rs\n  2-6 calcular_frete (2)\nsrc/pedido.rs (mudado depois do mapa)\n  1-4 fechar_pedido (2)\nFora do corte, lugares: 3, arquivos: 1. Repita a busca para ver a lista inteira.\nSe este não for o lugar, use suas ferramentas padrões: `Grep`, `Glob` e `Read`.";
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

    /// A nota que traz o código do trecho editado dispensa a leitura desse
    /// trecho, e a saída inteira do `grep` fica no gasto, porque a busca roda;
    /// a leitura de outro arquivo fica, e a nota de outro arquivo só soma.
    #[test]
    fn a_note_that_shows_the_edited_lines_saves_the_read_and_keeps_the_grep_output() {
        let row = row(chain_edited(), &["src/a.rs"]);
        let note = "n".repeat(500);
        let covering = Shown { files: vec!["src/a.rs".to_string()], ranges: vec![("src/a.rs".to_string(), 5, 30)] };
        let spend = spend_of(&row, &Outcome::Note(note.clone()), &covering).unwrap();
        assert_eq!((spend.today, spend.with, spend.saved_reads), (7000, 7000 + 500 - 4000, 4000));

        let short = Shown { files: vec!["src/a.rs".to_string()], ranges: vec![("src/a.rs".to_string(), 5, 15)] };
        let spend = spend_of(&row, &Outcome::Note(note.clone()), &short).unwrap();
        assert_eq!((spend.with, spend.saved_reads), (7000 + 500, 0));

        let other = Shown { files: vec!["src/b.rs".to_string()], ranges: vec![("src/b.rs".to_string(), 1, 50)] };
        let spend = spend_of(&row, &Outcome::Note(note.clone()), &other).unwrap();
        assert_eq!((spend.with, spend.saved_reads), (7000 + 500, 0));

        let spend = spend_of(&row, &Outcome::Note(note), &Shown::default()).unwrap();
        assert_eq!((spend.with, spend.saved_reads), (7000 + 500, 0));
    }

    /// Na cadeia sem edição, a nota que traz por inteiro o trecho lido dispensa
    /// essa leitura, pela mesma regra da resposta.
    #[test]
    fn without_an_edit_the_read_that_a_note_shows_whole_goes() {
        let mut chain = chain_edited();
        chain.ended = "usuario".to_string();
        chain.edit_file = None;
        chain.edit_range = None;
        chain.reads[0] = Read { file: "src/a.rs".to_string(), first: 10, last: 40, chars: 1500 };
        chain.reads_chars = 3500;
        let row = row(chain, &["src/a.rs"]);
        let shown = Shown { files: vec!["src/a.rs".to_string()], ranges: vec![("src/a.rs".to_string(), 8, 60)] };
        let spend = spend_of(&row, &Outcome::Note("z".repeat(200)), &shown).unwrap();
        assert_eq!((spend.today, spend.with, spend.saved_reads), (4500, 4500 + 200 - 1500, 1500));
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

    /// A nota parcial que o gancho dá de verdade, lida pela régua: o arquivo
    /// certo que ela traz vira a posição dele no termômetro, e o código que ela
    /// mostra dispensa a leitura do trecho editado, sem tirar a saída do `grep`.
    #[test]
    fn the_partial_note_the_hook_gives_is_read_for_its_file_and_its_code() {
        let (_dir, root) = fixture::repo("{}");
        let mut chain = chain_edited();
        chain.edit_file = Some("src/frete.rs".to_string());
        chain.edit_range = Some((3, 4));
        chain.reads[0] = Read { file: "src/frete.rs".to_string(), first: 1, last: 40, chars: 4000 };
        let search = Row { tool_input: json!({ "command": "grep -rn imposto src" }), ..row(chain, &["src/frete.rs"]) };

        let (outcome, _) = hear(&root, &search, "s-nota-parcial");
        let Outcome::Note(note) = &outcome else { panic!("a note was expected, got {outcome:?}") };
        let shown = shown_by(&outcome);
        assert_eq!(shown.files.first().map(String::as_str), Some("src/frete.rs"), "{note}");
        assert!(covers(&shown, "src/frete.rs", 3, 4), "{shown:?}\n{note}");
        assert_eq!(fate_of(&root, &search, (&outcome, &shown)), Fate::At(1));

        let spend = spend_of(&search, &outcome, &shown).unwrap();
        assert_eq!((spend.today, spend.saved_reads), (7000, 4000));
        assert_eq!(spend.with, 7000 + note.chars().count() - 4000, "the grep output stays in the spend");
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
        let other = Shown { files: vec!["src/frete.rs".to_string()], ranges: vec![] };
        assert_eq!(fate_of(&root, &pedido, (&Outcome::Note("n".to_string()), &other)), Fate::Passed);
        let with_file = Shown { files: vec!["src/frete.rs".to_string(), "src/pedido.rs".to_string()], ranges: vec![] };
        assert_eq!(fate_of(&root, &pedido, (&Outcome::Note("n".to_string()), &with_file)), Fate::At(2));

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

    /// Um mapa gravado como o scan grava, com `mark` em cada bloco.
    fn map_marked(mark: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let model = store::model_path(dir.path());
        let map = json!({ "modules": [{ "path": "src/a.rs", "language": "rust", "loc": 1, "declarations": [
            { "kind": "function", "name": "a", "line": 1, "end_line": 1 }] }] });
        store::save_at(&model, &map, mark, &mustard_core::domain::normalize::Languages::of_project(dir.path())).expect("map saved");
        (dir, model)
    }

    /// A prova de um programa de mentira, com o commit e o resumo que a medida diz.
    fn proof_of_the_measure() -> mustard_core::io::measure_proof::MeasureProof {
        mustard_core::io::measure_proof::MeasureProof {
            commit: "0123456789ab".to_string(),
            dirty: false,
            diff: String::new(),
            binary_sha256: "0".repeat(64),
            binary_path: "programa".to_string(),
            maps: Vec::new(),
        }
    }

    /// A porta comum recusa o mapa de marca diferente da que o scan compilado
    /// diz, antes de qualquer medida, e o mapa de marca igual passa e entra na
    /// prova, um por mapa aberto.
    #[test]
    fn measure_gate_refuses_a_map_of_another_mark_and_records_the_ones_that_pass() {
        let (_one, same) = map_marked("scan 1");
        let (_two, other) = map_marked("scan 2");

        let gate = MeasureGate::with(proof_of_the_measure(), "scan 1".to_string(), None).expect("the gate opens");
        let refused = check_maps(gate, &[same.clone(), other]).expect_err("the map of another mark stops the ruler").to_string();
        assert!(refused.contains("marca do mapa scan 2, o código compilado produz scan 1"), "{refused}");

        let gate = MeasureGate::with(proof_of_the_measure(), "scan 1".to_string(), None).expect("the gate opens");
        let gate = check_maps(gate, std::slice::from_ref(&same)).expect("the map of the same mark passes");
        let maps = &gate.proof().maps;
        assert_eq!(maps.len(), 1, "one entry per opened map");
        assert_eq!(maps[0].pieces.len(), 6, "the map carries the state of every piece of the search");
        assert_eq!((maps[0].path.as_str(), maps[0].mark.as_str()), (same.to_str().expect("a path"), "scan 1"));
    }

    /// Rodar a régua direto, sem o comando de medida, recusa com a frase que
    /// diz o caminho: a prova não pode ficar em branco.
    #[test]
    #[should_panic(expected = "rode pelo comando de medida")]
    fn measure_gate_refuses_to_run_without_the_measure_command() {
        let _ = measure_gate(&[]);
    }

    /// O programa compilado de um commit que não é o da medida não passa na
    /// porta: o número seria de outro código.
    #[test]
    fn measure_gate_refuses_a_program_built_from_another_commit_than_the_measurement_says() {
        let mut claims_another = proof_of_the_measure();
        claims_another.commit = "ffffffffffff".to_string();
        let refused = MeasureGate::with(claims_another, "scan 1".to_string(), Some(&built_stamp()))
            .expect_err("the program was not built from that commit")
            .to_string();
        assert!(refused.contains("compilado"), "{refused}");
    }
}
