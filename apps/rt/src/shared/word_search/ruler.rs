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
//! A busca real leva ao filtro a última fala do agente antes da chamada. Por
//! isso a linha da entrada traz também `session`, o arquivo da conversa, e
//! `at`, o instante da chamada, em segundos desde 1970 em UTC, como o arquivo
//! de buscas o grava: a régua grava numa pasta temporária a conversa só com as
//! linhas escritas até o fim desse segundo, e o gancho a lê como o de uma
//! sessão. Uma fala escrita depois nunca chega ao filtro. Sem o arquivo da
//! sessão, ou com ele sem fala do agente antes da chamada (a fala que o leitor
//! do gancho acha nele), a busca segue sem fala, e o resultado conta quantas
//! ficaram sem.
//!
//! O filtro (o Jev) roda como no produto: com a chave no ambiente, a busca
//! parcial o chama. A régua grava, por busca, se ele foi chamado, se falhou e
//! por quê, os tokens, o custo, as peças que ele guardou e as que as ligações
//! puxaram, e cada grupo soma. A primeira busca de cada mapa também o chama: a
//! soma dela sai numa linha à parte.
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
//! A busca que não tem como acertar fica de fora do acerto: o arquivo certo nem
//! está no mapa, ou, quando o filtro foi chamado, não está entre os candidatos
//! que foram a ele. A linha de cada grupo traz o acerto sobre as buscas que
//! tinham como acertar.
//!
//! As duas metades da entrada (`A` e `B`) vêm separadas por conversa: uma
//! escolhe, a outra confere. Cada projeto sai em tabelas de cada metade e do
//! total.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::DateTime;

use mustard_core::domain::model::contract::{HookInput, Trigger, Verdict};
use mustard_core::io::map_triage::Triaged;
use mustard_core::io::project_map::{self as store, Need};
use mustard_core::platform::error::Result as CoreResult;
use serde::Deserialize;
use serde_json::{Value, json};

use self::gate::MeasureGate;
use self::jev::{JevSum, JevUse};
use super::{FileHits, SHOWN_FILES};
use crate::shared::agent_said;

/// A prova de versão do número e a conferência dos mapas que a régua abre.
mod gate;

/// O que o filtro fez em cada busca, e a soma dele.
pub(crate) mod jev;

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
    /// O arquivo da conversa em que a busca aconteceu; sem ele, a busca segue
    /// sem fala.
    #[serde(default)]
    session: Option<String>,
    /// O instante da chamada, em segundos desde 1970 em UTC, como o arquivo de
    /// buscas o grava: a conversa que o filtro vê vai até o fim desse segundo.
    #[serde(default)]
    at: Option<i64>,
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
            let Some(path) = current.clone() else {
                continue;
            };
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
                right.contains(&read.file.as_str()) && read.first > 0 && covers(shown, &read.file, read.first as u64, read.last.max(read.first) as u64)
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

/// O arquivo certo da busca de `row` está no mapa de `root`: algum dos
/// `targets` é módulo dele.
fn target_in_map(root: &Path, row: &Row) -> bool {
    let model = store::model_path(root);
    store::read_for_at(&model, Need::Paths).is_ok_and(|paths| paths.modules.iter().any(|module| row.targets.contains(&module.path)))
}

/// Por que a resposta não traz nenhum dos arquivos `targets`: o que o gancho
/// ordenou na última busca (com a pasta pedida, como ele a leu) e o mapa.
fn cause_of(root: &Path, row: &Row) -> Fate {
    if !target_in_map(root, row) {
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

/// O segundo em que uma linha da conversa foi escrita (`timestamp`, em RFC
/// 3339, lido como segundos desde 1970 em UTC); `None` na linha que não é
/// mensagem ou não diz quando foi escrita.
fn instant_of(line: &[u8]) -> Option<i64> {
    let entry: Value = serde_json::from_slice(line).ok()?;
    DateTime::parse_from_rfc3339(entry.get("timestamp")?.as_str()?).ok().map(|when| when.timestamp())
}

/// A conversa da busca de `row` como estava no instante da chamada: só as
/// linhas do arquivo da sessão escritas até o fim do segundo em que a chamada
/// aconteceu (o instante chega em segundos inteiros, e a fala dita junto da
/// chamada cai no mesmo segundo), copiadas como estão e na ordem em que
/// estavam — nenhuma é reescrita nem criada. A linha escrita depois nunca
/// entra, porque a busca real não a conhecia; a que não diz quando foi escrita
/// também não. `None` sem arquivo da sessão que se leia e sem instante: a
/// busca segue sem fala.
fn conversation_before(row: &Row) -> Option<Vec<u8>> {
    let at = row.at?;
    let raw = std::fs::read(row.session.as_deref()?).ok()?;
    let kept = raw.split_inclusive(|byte| *byte == b'\n').filter(|line| instant_of(line).is_some_and(|when| when <= at)).flatten().copied().collect();
    Some(kept)
}

/// A conversa de `row` até a chamada ([`conversation_before`]) gravada em
/// `<scratch>/<session>.jsonl`, o arquivo que o gancho lê em `transcript_path`
/// como lê o de uma sessão de verdade; `None` quando a busca segue sem
/// conversa.
fn write_conversation(row: &Row, session: &str, scratch: &Path) -> Option<PathBuf> {
    let text = conversation_before(row)?;
    let file = scratch.join(format!("{session}.jsonl"));
    std::fs::write(&file, text).ok().map(|()| file)
}

/// O que o despachante do gancho respondeu a uma busca, quanto levou, se a
/// fala do agente chegou a ele e o que o filtro fez.
struct Heard {
    outcome: Outcome,
    took: Duration,
    /// A conversa da sessão chegou ao gancho em `transcript_path`.
    with_conversation: bool,
    /// A conversa trouxe fala do agente antes da chamada: a que o leitor do
    /// gancho acha nela, não vazia. Sem ela a busca foi sem fala.
    with_speech: bool,
    /// O que o filtro fez na busca.
    jev: JevUse,
}

/// O que o despachante do gancho responde à chamada de `row`, rodada de
/// `root` numa sessão só dela, e quanto ele levou. A conversa de `row` até a
/// chamada vai para a pasta temporária `scratch` ([`write_conversation`]) e o
/// gancho a lê em `transcript_path`, sem nome de subagente; a régua lê dela a
/// fala pelo mesmo leitor do gancho, e o arquivo sai ao fim da busca.
fn hear(root: &Path, row: &Row, session: &str, scratch: &Path) -> Heard {
    let conversation = write_conversation(row, session, scratch);
    let with_speech = conversation.as_deref().is_some_and(|file| !agent_said::last_said(file).is_empty());
    let input = HookInput {
        tool_name: Some(row.tool_name.clone()),
        tool_input: row.tool_input.clone(),
        hook_event_name: Some("PreToolUse".to_string()),
        cwd: Some(root.to_string_lossy().into_owned()),
        session_id: Some(session.to_string()),
        raw: conversation.as_ref().map_or(Value::Null, |file| json!({ "transcript_path": file })),
        ..HookInput::default()
    };
    LAST_ORDER.with(|last| *last.borrow_mut() = None);
    LAST_HITS.with(|last| last.borrow_mut().clear());
    jev::forget();
    let started = Instant::now();
    let verdict = crate::dispatch::run_event(Some(Trigger::PreToolUse), &input).verdict;
    let took = started.elapsed();
    if let Some(file) = &conversation {
        let _ = std::fs::remove_file(file);
    }
    let outcome = match verdict {
        Verdict::Deny { reason } => Outcome::Answer(reason),
        Verdict::Inject { context } => Outcome::Note(context),
        _ => Outcome::Pass,
    };
    Heard { outcome, took, with_conversation: conversation.is_some(), with_speech, jev: jev::take() }
}

/// Se o arquivo certo da busca tinha como aparecer: está no mapa e, quando o
/// filtro foi chamado, está entre os candidatos que foram a ele.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Reach {
    in_map: bool,
    /// O arquivo certo está entre os candidatos do filtro; `None` quando o
    /// filtro não foi chamado.
    in_candidates: Option<bool>,
}

impl Reach {
    /// A busca tinha como acertar.
    fn possible(self) -> bool {
        self.in_map && self.in_candidates != Some(false)
    }
}

/// O alcance da busca de `row` na raiz `root`: o mapa dela e os candidatos que
/// o filtro recebeu em `jev`.
fn reach_of(root: &Path, row: &Row, jev: &JevUse) -> Reach {
    Reach { in_map: target_in_map(root, row), in_candidates: jev.called.then(|| jev.sent.iter().any(|path| row.targets.contains(path))) }
}

/// A soma de um grupo de buscas.
#[derive(Debug, Default)]
struct Sum {
    searches: usize,
    with_chain: usize,
    today: usize,
    with: usize,
    saved_reads: usize,
    /// As buscas que foram sem fala do agente.
    without_speech: usize,
    /// As buscas em que o arquivo certo tinha como aparecer, e, entre elas, as
    /// que o mostraram.
    possible: usize,
    right: usize,
    /// As buscas que não tinham como acertar: o arquivo certo fora do mapa, e o
    /// que está no mapa mas ficou fora dos candidatos do filtro.
    outside_map: usize,
    outside_candidates: usize,
    jev: JevSum,
    fates: BTreeMap<String, usize>,
    millis: Vec<u128>,
}

impl Sum {
    /// Soma uma busca: o tempo, o destino, o gasto, quando a conversa tinha
    /// cadeia, se ela foi com fala do agente, se o arquivo certo tinha como
    /// aparecer e o que o filtro fez.
    fn record(&mut self, heard: &Heard, fate: Fate, spend: Option<Spend>, reach: Reach) {
        self.searches += 1;
        self.millis.push(heard.took.as_millis());
        *self.fates.entry(fate_name(fate)).or_default() += 1;
        if !heard.with_speech {
            self.without_speech += 1;
        }
        if reach.possible() {
            self.possible += 1;
            self.right += usize::from(matches!(fate, Fate::At(_)));
        } else if reach.in_map {
            self.outside_candidates += 1;
        } else {
            self.outside_map += 1;
        }
        self.jev.record(&heard.jev);
        if let Some(spend) = spend {
            self.with_chain += 1;
            self.today += spend.today;
            self.with += spend.with;
            self.saved_reads += spend.saved_reads;
        }
    }

    fn show(&self, label: &str) -> String {
        let percent = |part: usize, whole: usize| {
            if whole == 0 { 0.0 } else { 100.0 * part as f64 / whole as f64 }
        };
        let gain = self.today as i64 - self.with as i64;
        let mut millis = self.millis.clone();
        millis.sort_unstable();
        let at = |share: usize| millis.get((millis.len() * share / 100).min(millis.len().saturating_sub(1))).copied().unwrap_or(0);
        let answered: usize = self.fates.iter().filter(|(fate, _)| !fate.starts_with("passou")).map(|(_, n)| n).sum();
        let first = self.fates.get("1").copied().unwrap_or(0);
        let five: usize = (1..=SHOWN_FILES).map(|n| self.fates.get(&n.to_string()).copied().unwrap_or(0)).sum();
        let fates: Vec<String> = self.fates.iter().map(|(fate, n)| format!("{fate} {n}")).collect();
        format!(
            "{label}: {} buscas | gasto em {} com cadeia: hoje {} | com o Mustard {} | diferença {gain} ({:.1}%), leitura dispensada {} | responde {} ({:.1}%), certo em 1º {first}, entre 5 {five} ({:.1}% das que respondem) | destinos: {} | acerto sobre as possíveis {} de {} ({:.1}%), impossíveis {} (fora do mapa {}, fora dos candidatos do Jev {}) | {} | sem fala {} | tempo ms: mediana {}, p95 {}, máx {}",
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
            self.right,
            self.possible,
            percent(self.right, self.possible),
            self.outside_map + self.outside_candidates,
            self.outside_map,
            self.outside_candidates,
            self.jev.show(self.searches),
            self.without_speech,
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

/// Confere cada mapa de `maps` na porta `gate`, na ordem; o primeiro de
/// marca diferente da que o scan compilado diz recusa, e nenhum número sai.
fn check_maps(mut gate: MeasureGate, maps: &[PathBuf]) -> CoreResult<MeasureGate> {
    for map in maps {
        gate.check(map)?;
    }
    Ok(gate)
}

/// A porta comum das réguas do rt: a prova de versão deste programa e a
/// conferência dos mapas que a régua vai abrir, antes de ela medir. Com o
/// programa sem o commit no carimbo, sem o `scan` compilado ao lado ou com um
/// mapa de outra compilação do `scan`, a régua para aqui, dizendo por quê.
pub(super) fn measure_gate(maps: &[PathBuf]) -> MeasureGate {
    MeasureGate::open().and_then(|gate| check_maps(gate, maps)).unwrap_or_else(|refusal| panic!("a régua não mede: {refusal}"))
}

/// O que a régua tira de uma busca: o que o gancho respondeu, o que ele
/// mostra, o destino no termômetro, o gasto e se o arquivo certo tinha como
/// aparecer.
struct Measured {
    heard: Heard,
    shown: Shown,
    fate: Fate,
    spend: Option<Spend>,
    reach: Reach,
}

impl Measured {
    /// Roda a busca de `row` na raiz `root`, na sessão `session`, e a mede.
    fn of(root: &Path, row: &Row, session: &str, scratch: &Path) -> Self {
        let heard = hear(root, row, session, scratch);
        let shown = shown_by(&heard.outcome);
        let fate = fate_of(root, row, (&heard.outcome, &shown));
        let spend = spend_of(row, &heard.outcome, &shown);
        let reach = reach_of(root, row, &heard.jev);
        Self { heard, shown, fate, spend, reach }
    }

    /// A linha do resultado da busca, com a prova de versão `proof`. Lê a
    /// ordem e as linhas achadas que o gancho lembrou da busca que acabou de
    /// rodar.
    fn line(&self, row: &Row, proof: &Value) -> String {
        let (heard, shown, spend) = (&self.heard, &self.shown, self.spend);
        let (kind, text) = match &heard.outcome {
            Outcome::Answer(text) => ("answer", text.as_str()),
            Outcome::Note(text) => ("note", text.as_str()),
            Outcome::Pass => ("pass", ""),
        };
        let mut line = json!({
            "key": row.key, "project": row.project, "half": row.half, "at": row.at, "outcome": kind, "millis": heard.took.as_millis(),
            "fate": fate_name(self.fate), "shown": shown.files, "ranges": shown.ranges, "chars": text.chars().count(),
            "with_conversation": heard.with_conversation, "with_speech": heard.with_speech,
            "today": spend.map(|s| s.today), "with": spend.map(|s| s.with), "saved_reads": spend.map(|s| s.saved_reads),
            "chain": row.chain.as_ref().map(|c| c.ended.as_str()), "text": text,
            "target_in_map": self.reach.in_map, "target_in_candidates": self.reach.in_candidates,
            "hit_files": LAST_HITS.with(|last| last.borrow().len()),
            "target_hits": LAST_HITS.with(|last| last.borrow().iter().filter(|(path, _)| row.targets.contains(path)).cloned().collect::<Vec<_>>()),
            "target_order": LAST_ORDER.with(|last| last.borrow().as_ref().and_then(|order| order.iter().position(|path| row.targets.contains(path)))),
            "order_len": LAST_ORDER.with(|last| last.borrow().as_ref().map(Vec::len)),
            "proof": proof,
        });
        if let Value::Object(fields) = &mut line {
            fields.extend(heard.jev.fields());
        }
        line.to_string()
    }
}

/// A rodada da régua: uma linha de resultado por busca, a soma de cada grupo,
/// o filtro nas buscas de aquecimento, que não entram em nenhuma conta, e o
/// que ficou de fora.
#[derive(Default)]
struct Round {
    lines: Vec<String>,
    groups: BTreeMap<(String, String), Sum>,
    /// O filtro nas buscas de aquecimento, somado à parte.
    warmup: JevSum,
    /// Os mapas já aquecidos.
    warmed: Vec<String>,
    /// As buscas vencidas e as que não são busca de texto, fora das contas.
    skipped: usize,
    other: usize,
    /// As buscas que foram sem fala do agente.
    without_speech: usize,
}

impl Round {
    /// Mede as buscas de `rows`, cada uma na raiz que `root_of` diz, com a
    /// prova de versão `proof` em cada linha. As vencidas (o texto já não casa
    /// com o arquivo certo) e as que não são do `Bash` nem do `Grep` ficam de
    /// fora.
    fn measure(rows: &[Row], root_of: impl Fn(&Row) -> PathBuf, proof: &Value) -> Self {
        let mut round = Self::default();
        // A conversa de cada busca, só até a chamada, mora aqui enquanto ela roda.
        let scratch = tempfile::tempdir().expect("the folder for the conversations of the searches");
        // A sessão de cada busca é só dela e só desta medida: o gancho lembra a busca repetida numa sessão e a deixaria passar.
        let run = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_millis());
        for (at, row) in rows.iter().enumerate() {
            if row.expired {
                round.skipped += 1;
                continue;
            }
            if !matches!(row.kind.as_str(), "bash" | "grep") {
                round.other += 1;
                continue;
            }
            let root = root_of(row);
            if !round.warmed.contains(&row.name) {
                round.warmed.push(row.name.clone());
                // A primeira busca de cada mapa refaz o índice nas línguas dele: fica fora do tempo e das contas, e o que o filtro gasta nela se soma à parte.
                let warm = hear(&root, row, &format!("warm-{run}-{at}"), scratch.path());
                round.warmup.record(&warm.jev);
            }
            let measured = Measured::of(&root, row, &format!("spend-{run}-{at}"), scratch.path());
            round.without_speech += usize::from(!measured.heard.with_speech);
            for half in [row.half.as_str(), "total"] {
                round.groups.entry((row.project.clone(), half.to_string())).or_default().record(&measured.heard, measured.fate, measured.spend, measured.reach);
            }
            round.lines.push(measured.line(row, proof));
        }
        round
    }

    /// As linhas `GASTO` da rodada: o que ficou fora, cada grupo e o
    /// aquecimento.
    fn report(&self) -> Vec<String> {
        let mut report = vec![format!(
            "GASTO {} buscas vencidas fora, {} sem busca de texto (glob, explore, mustard) fora, {} buscas sem fala do agente (a conversa da sessão não chegou, ou chegou sem fala)",
            self.skipped, self.other, self.without_speech
        )];
        for ((project, half), sum) in &self.groups {
            report.push(format!("GASTO {}", sum.show(&format!("{project} {half}"))));
        }
        report.push(format!("GASTO aquecimento (a primeira busca de cada mapa, fora das contas): {}", self.warmup.show(self.warmed.len())));
        report
    }
}

/// A régua do gasto. A entrada é `SPEND_INPUT` (o arquivo de buscas do
/// preparo) e as cópias dos projetos, cada uma com o mapa e o `mustard.json` do
/// projeto real, estão em `SPEND_TREES/<nome>`; `SPEND_OUT` recebe uma linha
/// por busca, cada uma com a prova de versão em `proof`, que diz o commit de
/// que o programa foi compilado, o que faltava comitar, o hash do programa e a
/// marca de cada mapa aberto. Só as buscas do `Bash` e do `Grep` entram nas
/// contas; as vencidas (o texto já não casa com o arquivo certo na cópia) ficam
/// de fora.
///
/// Roda em `--release`, para o tempo ser o do gancho de verdade, com o `scan`
/// do mesmo build ao lado (`cargo build --release -p scan`) e com o mapa de
/// cada cópia feito por ele: a régua recusa o mapa de outra marca. A busca roda
/// como a do produto, com o Jev: a chave vai em `TYPESAFE_API_KEY` no ambiente,
/// nunca tirada dele. O programa de teste é o que o cargo diz ao compilar
/// (`cargo test --release -p mustard-rt --lib --no-run`), nunca o mais novo da
/// pasta. Roda com `HOME` numa pasta vazia, para a busca não ler a pasta do
/// usuário, e com `$HOME` (nunca `~`, que depois do `=` o terminal não troca)
/// abrindo os caminhos:
/// `SPEND_INPUT=$HOME/.cache/mustard-medida/regua-gasto/entrada.json
/// SPEND_TREES=$HOME/.cache/mustard-medida/regua-gasto/src
/// SPEND_OUT=$HOME/.cache/mustard-medida/regua-gasto/saida.json
/// HOME=$HOME/.cache/mustard-medida/regua-gasto/home <programa de teste>
/// measure_the_spend_of_the_search --ignored --nocapture`.
#[test]
#[ignore = "mede com as cópias dos projetos de prova e as conversas reais"]
fn measure_the_spend_of_the_search() {
    let input = std::env::var("SPEND_INPUT").expect("SPEND_INPUT points to the searches file");
    let trees = PathBuf::from(std::env::var("SPEND_TREES").expect("SPEND_TREES points to the folder of the project copies"));
    let out = std::env::var("SPEND_OUT").expect("SPEND_OUT points to the file to write");
    let rows: Vec<Row> = serde_json::from_str(&std::fs::read_to_string(input).expect("the searches file reads")).expect("the searches file parses");
    // Os mapas das cópias que entram nas contas, conferidos antes da primeira busca.
    let mut names: Vec<&str> = rows.iter().filter(|row| !row.expired && matches!(row.kind.as_str(), "bash" | "grep")).map(|row| row.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    let maps: Vec<PathBuf> = names.iter().map(|name| store::model_path(&trees.join(name))).collect();
    let gate = measure_gate(&maps);
    let round = Round::measure(&rows, |row| trees.join(&row.name), &gate.proof().to_json());
    std::fs::write(out, round.lines.join("\n")).expect("the rows file writes");
    for line in round.report() {
        eprintln!("{line}");
    }
    eprintln!("{}", gate.proof().line());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::word_search::fixture::{self, Judge};

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

    /// O instante da chamada nos testes, como o arquivo de buscas o grava:
    /// 2026-10-01T10:00:05Z, em segundos desde 1970.
    const CALL: i64 = 1_790_848_805;

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
            session: None,
            at: None,
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

    /// Uma linha da conversa que o Claude Code guarda: o papel, o instante e os
    /// blocos da mensagem.
    fn line(role: &str, at: &str, blocks: Value) -> Value {
        json!({"type": role, "timestamp": at, "message": {"role": role, "content": blocks}})
    }

    fn said(at: &str, text: &str) -> Value {
        line("assistant", at, json!([{"type": "text", "text": text}]))
    }

    fn called(at: &str) -> Value {
        line("assistant", at, json!([{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "grep -rn imposto src"}}]))
    }

    /// O arquivo da conversa com as linhas dadas, uma por linha do arquivo.
    fn session_file(dir: &Path, lines: &[Value]) -> String {
        let file = dir.join("sessao.jsonl");
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        std::fs::write(&file, text.join("\n") + "\n").expect("the session file");
        file.to_string_lossy().into_owned()
    }

    /// O arquivo que a régua grava para o gancho é o da sessão até a chamada,
    /// linha por linha como estava (espaços, ordem das chaves e fim de linha
    /// intactos), e o leitor da fala do gancho acha nele a última fala
    /// anterior à chamada.
    #[test]
    fn the_file_written_for_the_hook_keeps_the_lines_as_they_were_and_the_reader_finds_the_last_speech_before_the_call() {
        let notes = tempfile::tempdir().expect("a folder");
        let scratch = tempfile::tempdir().expect("a folder");
        let before = [
            r#"{"type":"user","timestamp":"2026-10-01T10:00:00.000Z","message":{"role":"user","content":"ache o cálculo do imposto"}}"#,
            r#"{ "timestamp" : "2026-10-01T10:00:03.000Z",  "message": {"content": [ {"text": "Fala antiga.", "type": "text"} ], "role": "assistant"} }"#,
            r#"{"timestamp":"2026-10-01T10:00:04.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Vou ver onde o imposto é calculado."}]}}"#,
            r#"{"timestamp":"2026-10-01T10:00:05.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"grep -rn imposto src"}}]}}"#,
        ];
        let after = [
            r#"{"timestamp":"2026-10-01T10:00:07.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Fala de depois da chamada."}]}}"#,
            r#"{"summary":"linha sem instante","message":{"role":"assistant","content":[{"type":"text","text":"Fala sem instante."}]}}"#,
        ];
        let whole: Vec<&str> = before.iter().chain(&after).copied().collect();
        let file = notes.path().join("sessao.jsonl");
        std::fs::write(&file, whole.join("\n") + "\n").expect("the session file");
        let search = Row { session: Some(file.to_string_lossy().into_owned()), at: Some(CALL), ..bash_row("grep -rn imposto src", &["src/frete.rs"]) };

        let written = write_conversation(&search, "s-arquivo", scratch.path()).expect("the conversation is written");

        assert_eq!(written.parent(), Some(scratch.path()));
        assert_eq!(std::fs::read_to_string(&written).expect("the file reads"), before.join("\n") + "\n");
        assert_eq!(agent_said::last_said(&written), "Vou ver onde o imposto é calculado.");
    }

    /// O instante lido como o arquivo de buscas o grava, um número em segundos
    /// desde 1970, corta a conversa no segundo certo; a data escrita não se
    /// lê: o formato é um só.
    #[test]
    fn an_instant_written_as_a_number_is_read_and_cuts_the_conversation_at_that_second() {
        let notes = tempfile::tempdir().expect("a folder");
        let session = session_file(
            notes.path(),
            &[
                said("2026-10-01T10:00:04.000Z", "Antes."),
                said("2026-10-01T10:00:05.999Z", "No segundo da chamada."),
                said("2026-10-01T10:00:06.000Z", "Depois."),
            ],
        );
        let search = |at: Value| {
            json!({ "key": "p|bash|1", "project": "p", "name": "p", "kind": "bash", "tool_name": "Bash", "tool_input": {}, "half": "A",
                    "targets": ["src/frete.rs"], "expired": false, "chain": null, "session": session, "at": at })
        };

        let row: Row = serde_json::from_value(search(json!(CALL))).expect("a row with a numeric instant reads");
        assert_eq!(row.at, Some(CALL));
        let kept = String::from_utf8(conversation_before(&row).expect("the conversation is cut")).expect("text");
        assert_eq!(kept.matches("\"type\":\"assistant\"").count(), 2, "{kept}");
        assert!(kept.contains("No segundo da chamada.") && !kept.contains("Depois."), "{kept}");

        assert!(serde_json::from_value::<Row>(search(json!("2026-10-01T10:00:05Z"))).is_err(), "the date written as text is not a format of the searches file");
        let without: Row = serde_json::from_value(search(Value::Null)).expect("a row without the instant reads");
        assert!(conversation_before(&without).is_none());
    }

    /// A conversa que a régua grava para a busca mora só enquanto ela roda: a
    /// pasta de trabalho fica vazia depois.
    #[test]
    fn the_conversation_written_for_a_search_does_not_outlive_it() {
        let (_dir, root) = fixture::repo("{}");
        let notes = tempfile::tempdir().expect("a folder");
        let session = session_file(notes.path(), &[said("2026-10-01T10:00:04.000Z", "Fala."), called("2026-10-01T10:00:05.000Z")]);
        let search = Row { session: Some(session), at: Some(CALL), ..bash_row("grep -rn imposto src", &["src/frete.rs"]) };
        let scratch = tempfile::tempdir().expect("a folder");

        let heard = hear(&root, &search, "s-pasta", scratch.path());

        assert!(heard.with_conversation);
        assert_eq!(std::fs::read_dir(scratch.path()).expect("the folder reads").count(), 0);
    }

    /// A busca ouvida de mentira, sem filtro: o tempo, a conversa e a fala.
    fn heard_with(with_conversation: bool, with_speech: bool) -> Heard {
        Heard { outcome: Outcome::Pass, took: Duration::from_millis(3), with_conversation, with_speech, jev: JevUse::default() }
    }

    /// O alcance de uma busca que tinha como acertar.
    const WITHIN_REACH: Reach = Reach { in_map: true, in_candidates: None };

    /// O resultado de cada grupo conta as buscas que foram sem fala do agente:
    /// a que não teve conversa e a que teve conversa sem fala.
    #[test]
    fn the_result_counts_the_searches_that_went_without_speech() {
        let mut sum = Sum::default();
        sum.record(&heard_with(false, false), Fate::Passed, None, WITHIN_REACH);
        sum.record(&heard_with(true, true), Fate::Passed, None, WITHIN_REACH);
        sum.record(&heard_with(true, false), Fate::Passed, None, WITHIN_REACH);
        assert_eq!((sum.searches, sum.without_speech), (3, 2));
        assert!(sum.show("p").contains("sem fala 2"), "{}", sum.show("p"));
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

    /// A prova de um programa de mentira, com o commit e o resumo dele.
    fn proof_of_the_measure() -> gate::MeasureProof {
        gate::MeasureProof {
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

        let gate = MeasureGate::with(proof_of_the_measure(), "scan 1".to_string());
        let refused = check_maps(gate, &[same.clone(), other]).expect_err("the map of another mark stops the ruler").to_string();
        assert!(refused.contains("marca do mapa scan 2, o código compilado produz scan 1"), "{refused}");

        let gate = MeasureGate::with(proof_of_the_measure(), "scan 1".to_string());
        let gate = check_maps(gate, std::slice::from_ref(&same)).expect("the map of the same mark passes");
        let maps = &gate.proof().maps;
        assert_eq!(maps.len(), 1, "one entry per opened map");
        assert_eq!((maps[0].path.as_str(), maps[0].mark.as_str()), (same.to_str().expect("a path"), "scan 1"));
    }

    /// O resultado que a régua grava leva, em cada linha, a prova de versão: o
    /// commit, o resumo do que faltava comitar e a marca de cada mapa aberto.
    #[test]
    fn every_result_line_carries_the_version_proof_and_the_mark_of_each_map() {
        let (_dir, root) = fixture::repo("{}");
        let (_one, first) = map_marked("scan 1");
        let (_two, second) = map_marked("scan 1");
        let mut proof = proof_of_the_measure();
        proof.dirty = true;
        proof.diff = "feedc0ffee12".to_string();
        let gate = MeasureGate::with(proof, "scan 1".to_string());
        let gate = check_maps(gate, &[first.clone(), second]).expect("both maps pass");

        let judge = Judge::sure_of(&[("calcular_frete", 0.9)]);
        let rows = [bash_row("grep -rn calcular_frete src", &["src/frete.rs"]), bash_row("grep -rn imposto src", &["src/frete.rs"])];
        let round = judge.installed(|| Round::measure(&rows, |_| root.clone(), &gate.proof().to_json()));

        assert_eq!(round.lines.len(), 2);
        for line in &round.lines {
            let result: Value = serde_json::from_str(line).expect("a result line is JSON");
            let proof = &result["proof"];
            assert_eq!(
                (proof["commit"].as_str(), proof["dirty"].as_bool(), proof["diff"].as_str()),
                (Some("0123456789ab"), Some(true), Some("feedc0ffee12")),
                "{result}"
            );
            assert_eq!(proof["maps"].as_array().map(Vec::len), Some(2), "{result}");
            assert_eq!(proof["maps"][0]["path"], json!(first.display().to_string()));
            assert_eq!(proof["maps"][1]["mark"], json!("scan 1"), "{result}");
        }
    }
}
