//! Historical comparison fixture. Compiled only by tests; never a product search path.
//! `word_search` — a resposta do mapa no lugar da busca por palavra.
//!
//! O `Grep` do Claude e o `grep`/`rg` do terminal, numa pasta de código do
//! projeto, recebem uma de três marcas, dadas pela triagem do mapa:
//!
//! - **cravado** — o primeiro arquivo tem a nota mais alta e a chance do
//!   corte medido na régua, mesmo com palavras da busca fora dos campos
//!   fortes dele: a resposta cita só as que ele traz;
//! - **parcial** — o mapa achou parte: os candidatos vão ao filtro (o Jev)
//!   pela porta única da busca ([`crate::shared::search_door`]), e a nota
//!   traz só as peças que ele entrega; sem chave, com o filtro desligado ou
//!   falhando, vale a nota da triagem, que diz quais palavras faltam, e o
//!   aviso do motivo sai uma vez por sessão;
//! - **não achou** — o mapa não achou nada, ou o filtro disse que nenhum
//!   candidato serve: a busca comum roda, e uma linha diz as palavras já
//!   quebradas e a próxima busca, exata.
//!
//! O cravado toma o lugar da busca comum, sem chamar o filtro. O parcial nunca
//! bloqueia: a busca comum roda, e a nota do mapa vai junto do resultado dela,
//! só como contexto, sem liberar nenhuma permissão. A busca que só lista nomes
//! de arquivo ou conta (`-l`, `-c`) roda como veio, com uma linha só da marca,
//! e não fica guardada como respondida: a que mostra linhas depois ainda
//! recebe a resposta. A resposta roda a mesma busca (mesmo padrão, mesmas
//! pastas) nos arquivos que o git conhece e agrupa o que achou pelas funções
//! do mapa: o arquivo, a linha de começo e a de fim de cada função, com as
//! linhas achadas dentro dela e o código dela, numerado, até um teto
//! ([`SNIPPET_LINES_PER_FN`], [`SNIPPET_BUDGET`]). A linha fora de função
//! conhecida (json, markdown) vem solta. Os arquivos saem na ordem da triagem
//! e ficam só os primeiros; o resto vira uma linha com a contagem de lugares e
//! de arquivos.
//!
//! A mesma busca repetida na sessão passa: o padrão e a pasta respondidos
//! ficam num estado curto da sessão. O arquivo que mudou depois da passada
//! do mapa (o conteúdo difere do que o mapa leu) é relido só para esta
//! resposta, e as linhas do mapa se levam para as dele; o arquivo vem
//! marcado como mudado. Numa cópia de trabalho, a árvore lida é a da cópia.
//!
//! A busca por nome de arquivo (`Glob`, `find -name`) tem a mesma marca, sem
//! resposta: as palavras do padrão de nome vão à triagem ([`name_words`]), e a
//! busca roda como veio, com a linha da marca. O pedido a um agente de
//! exploração vai à triagem como frase e palavras ([`ask_reply`]): só o
//! cravado sobe ao topo do pedido, como a resposta curta do mapa (arquivo,
//! função e linhas). O parcial e o que o mapa não achou deixam o pedido como
//! veio: a lista parcial só gasta leitura do agente e o desvia, e o filtro
//! não é chamado.
//!
//! Nunca falha: sem mapa, sem sessão, com regex que esta leitura não entende
//! ou passando do tempo, a resposta é passar, e a busca comum segue.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use mustard_core::ProjectConfig;
use mustard_core::domain::map_filter::Verdict;
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::project_map::{self, FilePart, FileParts};
use mustard_core::domain::triage::{Mark, not_found};
use mustard_core::io::fs;
use mustard_core::io::map_triage::{self, Triaged};
use mustard_core::io::project_map::{self as store, Need};
use mustard_core::platform::git;
use mustard_core::platform::i18n::Locale;
use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value, json};

use crate::shared::code_route::{ProjectPath, admitted, holds_code, parts_in_copy};
use crate::shared::config_key::{CONFIG_FILE, NameFilter, Walk};
use crate::shared::paths::{is_artifact, sensitive_pattern};
use crate::shared::say::say;
use crate::shared::search_door::{self as door, Ask, Assemble, Numbers, Outcome, Piece};

/// Quantos arquivos a resposta mostra: os primeiros da triagem. É o corte da
/// busca por assunto do mapa.
const SHOWN_FILES: usize = 5;

/// Quantos arquivos a triagem ordena para a resposta.
const RANKED_FILES: usize = 40;

/// Quantas linhas achadas dentro de uma função a resposta lista.
const HITS_SHOWN: usize = 3;

/// Quantas entradas (funções e linhas soltas) a resposta mostra por arquivo.
const ENTRIES_PER_FILE: usize = 8;

/// Quantos caracteres a linha solta mostra.
const LOOSE_WIDTH: usize = 100;

/// Quantas linhas da primeira função a resposta traz, numeradas, sob a entrada
/// dela; a função mais longa é cortada, com a contagem do que ficou de fora.
const SNIPPET_LINES_PER_FN: usize = 40;

/// O teto de caracteres do código da resposta, que só a primeira função
/// traz: passado ele, essa função também vem só com a linha dela.
const SNIPPET_BUDGET: usize = 5000;

/// O teto de caracteres da resposta inteira, com o código e os avisos: abaixo
/// dos 10 mil que o gancho aceita de uma vez.
const ANSWER_LIMIT: usize = 9000;


/// O maior arquivo lido, em bytes; o maior que isso é dado, não código.
const MAX_FILE_BYTES: u64 = 1_000_000;

/// Quantas buscas o estado da sessão guarda.
const REMEMBERED: usize = 200;

/// O que o gancho diz da busca.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reply {
    /// Nada a dizer: a busca comum roda como veio.
    Pass,
    /// A busca comum roda, com esta nota junto.
    Note(String),
    /// Esta resposta vale no lugar da busca comum.
    Answer(String),
}

/// O jeito como o programa lê o padrão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    /// A expressão do `rg`, da ferramenta de busca e do `grep -P`.
    Rust,
    /// A expressão básica do `grep`: `\|`, `\(`, `\)` e `\{` são operadores.
    Basic,
    /// O texto como está, sem operador (`-F`).
    Fixed,
}

/// Uma busca por palavra em pastas, como o gancho a leu.
pub(crate) struct Search<'a> {
    /// Os padrões, um por `-e`; a busca casa com qualquer um.
    pub(crate) patterns: &'a [String],
    pub(crate) dialect: Dialect,
    pub(crate) ignore_case: bool,
    pub(crate) whole_word: bool,
    /// As pastas buscadas, todas da mesma árvore.
    pub(crate) folders: &'a [ProjectPath],
    /// Os filtros de nome de arquivo, de entrada e de saída, na ordem da
    /// linha.
    pub(crate) filters: &'a [NameFilter],
    pub(crate) walk: Walk,
    /// A busca mostra as linhas achadas; `false` na que só lista nomes de
    /// arquivo ou conta.
    pub(crate) shows_lines: bool,
}

/// Onde a busca roda e para quem.
pub(crate) struct Scene<'a> {
    /// A raiz do projeto.
    pub(crate) root: &'a Path,
    /// O mapa do projeto.
    pub(crate) model: &'a Path,
    /// O arquivo do estado da sessão; sem ele a busca repetida não se
    /// reconhece, e quem chama só o dá a uma sessão de verdade.
    pub(crate) memory: Option<&'a Path>,
    /// A sessão de quem busca, onde cada aviso sai uma vez.
    pub(crate) session: Option<&'a str>,
    pub(crate) lang: Locale,
    pub(crate) languages: &'a Languages,
    /// A configuração do projeto: o filtro e os números da busca.
    pub(crate) config: &'a ProjectConfig,
    /// A montagem do filtro da busca parcial, a mesma da busca por assunto.
    pub(crate) assemble: &'a Assemble<'a>,
    /// A gravação da chamada medida da busca parcial. Quem monta a cena a
    /// entrega, porque a gravação numa spec é de quem grava conversa, nunca
    /// desta parte compartilhada.
    pub(crate) record: &'a Record<'a>,
    /// A descrição que o agente deu à busca; vazia quando ele não deu.
    pub(crate) described: &'a str,
    /// A última fala do agente antes da busca, que só o filtro lê.
    pub(crate) said: Said<'a>,
}

/// De onde vem a última fala do agente antes da busca. A leitura do arquivo da
/// conversa só acontece quando o filtro é chamado: a busca que a triagem crava
/// não a paga.
#[derive(Clone, Copy)]
pub(crate) enum Said<'a> {
    /// A fala já dada, como a de `--said`; vazia quando não há.
    Given(&'a str),
}

impl Said<'_> {
    /// O texto da fala; vazio quando não há.
    fn text(&self) -> String {
        match self {
            Said::Given(text) => (*text).to_string(),
        }
    }
}

/// A gravação de uma chamada medida: a raiz do projeto, o comando, a spec
/// nomeada, a sessão, o instante em que a chamada começou, o relatório dela e
/// os campos da medida. Devolve o número do evento gravado, ou `None` quando
/// nada foi gravado.
pub(crate) type Record<'a> = dyn Fn(&Path, &str, Option<&str>, Option<&str>, Instant, &Value, Map<String, Value>) -> Option<u64> + 'a;

/// A gravação de quem não chama o filtro: nada é gravado.
pub(crate) fn unrecorded(_: &Path, _: &str, _: Option<&str>, _: Option<&str>, _: Instant, _: &Value, _: Map<String, Value>) -> Option<u64> {
    None
}

/// O que o gancho diz da busca `search`: a resposta no lugar dela, a linha
/// que vai junto dela ou nada.
pub(crate) fn reply(scene: &Scene<'_>, search: &Search<'_>) -> Reply {
    try_reply(scene, search).unwrap_or(Reply::Pass)
}

// ---------------------------------------------------------------------------
// O nome de arquivo e o pedido a um agente de exploração
// ---------------------------------------------------------------------------

fn try_reply(scene: &Scene<'_>, search: &Search<'_>) -> Option<Reply> {
    let words = words_of(search.patterns, search.dialect == Dialect::Fixed);
    if words.is_empty() || matches!(search.walk, Walk::Rg { unignored: true }) {
        return None;
    }
    let tree = &search.folders.first()?.tree;
    if search.folders.iter().any(|folder| &folder.tree != tree) {
        return None;
    }
    let rels: Vec<String> = search.folders.iter().map(|folder| folder.rel.clone()).collect();
    let paths = store::read_for_at(scene.model, Need::Paths).ok()?;
    if !holds_code(&paths, &rels, search.filters, search.walk) {
        return None;
    }
    let key = key_of(search);
    if scene.memory.is_some_and(|memory| remembers(memory, &key)) {
        return None;
    }
    let question = words.join(" ");
    let triaged = map_triage::triage_at_in(scene.model, (&question, ""), scene.languages, (RANKED_FILES, &rels)).ok()?;
    #[cfg(test)]
    ruler::remember_order(&triaged);
    let mark = triaged.mark();
    if mark == Mark::NotFound {
        remember(scene.memory, &key)?;
        return Some(Reply::Note(not_found(&question, &triaged.words, scene.lang)));
    }
    if !search.shows_lines {
        return Some(Reply::Note(names_line(mark, &triaged, scene.lang)));
    }
    let regex = pattern_of(search)?;
    let hits = scan(tree, &rels, search, &regex)?;
    #[cfg(test)]
    ruler::remember_hits(&hits);
    // Só o parcial vai ao filtro: o cravado responde da triagem.
    let mut warnings: Vec<String> = Vec::new();
    let judged = if mark == Mark::Partial {
        // Num projeto com texto e código em línguas diferentes, as palavras
        // vão também como a frase, na língua do texto, para o filtro ler a
        // palavra como ela é e não como pedaço de nome.
        let intent = if scene.languages.codes().len() > 1 { question.as_str() } else { "" };
        judge(scene, (&question, intent), &triaged, (&rels, search), &mut warnings)
    } else {
        Judged::Triage
    };
    if judged == Judged::NotFound {
        remember(scene.memory, &key)?;
        return Some(Reply::Note(not_found(&question, &triaged.words, scene.lang)));
    }
    let delivered = match &judged {
        Judged::Pieces(pieces) => pieces.as_slice(),
        _ => &[],
    };
    let text = compose(scene, tree, (&triaged, mark), &hits, delivered, &warnings)?;
    remember(scene.memory, &key)?;
    // Só o cravado toma o lugar da busca; o parcial vai junto dela, sem
    // bloquear.
    Some(if mark == Mark::Pinned { Reply::Answer(text) } else { Reply::Note(text) })
}

/// O que o filtro disse da busca parcial.
#[derive(Debug, Clone, PartialEq)]
enum Judged {
    /// Sem filtro, sem candidato ou com o filtro falhando: vale a triagem.
    Triage,
    /// Nenhum candidato serve: a busca comum segue.
    NotFound,
    /// As peças que o filtro entrega, na ordem da chance.
    Pieces(Vec<Piece>),
}

/// A busca parcial pela porta única: as palavras de `question` são o pedido, e
/// `intent` é a frase de quem procura, quando há. Os candidatos e as peças são
/// só das pastas `rels` e dos tipos de arquivo de `search`. O motivo de não
/// haver filtro, ou de ele falhar, entra em `warnings`, uma vez por sessão.
fn judge(
    scene: &Scene<'_>,
    (question, intent): (&str, &str),
    triaged: &Triaged,
    (rels, search): (&[String], &Search<'_>),
    warnings: &mut Vec<String>,
) -> Judged {
    let (root, session, lang) = (scene.root, scene.session, scene.lang);
    let Some(assembled) = door::chosen_filter(root, session, lang, scene.config, scene.assemble, warnings) else {
        return Judged::Triage;
    };
    let numbers = Numbers::read(root, session, lang, scene.config, warnings);
    let said = scene.said.text();
    let ask = Ask {
        root,
        query: question,
        intent,
        described: scene.described,
        said: &said,
        lang,
        languages: scene.languages,
        numbers: &numbers,
        triaged,
        rels,
        filters: search.filters,
        walk: search.walk,
    };
    let Ok(classified) = door::classify(&ask, &assembled) else {
        return Judged::Triage;
    };
    #[cfg(test)]
    ruler::jev::remember_call(&classified);
    let judged = match classified.outcome {
        Outcome::NoCandidates => Judged::Triage,
        Outcome::Failed(error) => {
            door::failure_warning(root, session, lang, &error, warnings);
            Judged::Triage
        }
        Outcome::Classified { verdict: Verdict::NotFound, .. } => Judged::NotFound,
        Outcome::Classified { pieces, .. } if pieces.is_empty() => Judged::Triage,
        Outcome::Classified { pieces, .. } => Judged::Pieces(pieces),
    };
    let _ = (scene.record)(root, "word search", None, session, Instant::now(), &json!({ "ok": true }), classified.measured);
    judged
}

// ---------------------------------------------------------------------------
// As palavras e o padrão
// ---------------------------------------------------------------------------

pub(crate) use words::words_of;

/// A expressão básica do `grep` escrita como a do `regex`: os operadores
/// escapados viram operadores, e os sem escape viram texto.
fn basic_to_extended(pattern: &str) -> String {
    let mut out = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(next @ ('|' | '(' | ')' | '{' | '}' | '+' | '?')) => out.push(next),
                Some('<' | '>') => out.push_str(r"\b"),
                Some(next) => {
                    out.push('\\');
                    out.push(next);
                }
                None => out.push_str(r"\\"),
            },
            '|' | '(' | ')' | '{' | '}' | '+' | '?' => {
                out.push('\\');
                out.push(c);
            }
            '[' => {
                out.push(c);
                let mut inner = chars.clone().peekable();
                if inner.peek() == Some(&'^') {
                    out.extend(chars.next());
                    inner.next();
                }
                if inner.peek() == Some(&']') {
                    out.extend(chars.next());
                }
                while let Some(next) = chars.next() {
                    out.push(next);
                    if next == '[' && chars.clone().next() == Some(':') {
                        let mut previous = ' ';
                        for class in chars.by_ref() {
                            out.push(class);
                            if previous == ':' && class == ']' {
                                break;
                            }
                            previous = class;
                        }
                    } else if next == ']' {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// O padrão da busca como um `regex`; `None` no que ele não entende
/// (referência de volta, olhar adiante) ou no grande demais.
fn pattern_of(search: &Search<'_>) -> Option<Regex> {
    let parts: Vec<String> = search
        .patterns
        .iter()
        .map(|pattern| match search.dialect {
            Dialect::Fixed => regex::escape(pattern),
            Dialect::Basic => basic_to_extended(pattern),
            Dialect::Rust => pattern.clone(),
        })
        .map(|source| format!("(?:{source})"))
        .collect();
    let mut source = parts.join("|");
    if search.whole_word {
        source = format!(r"\b(?:{source})\b");
    }
    RegexBuilder::new(&source).multi_line(true).case_insensitive(search.ignore_case).size_limit(1 << 20).dfa_size_limit(4 << 20).build().ok()
}

// ---------------------------------------------------------------------------
// O estado da sessão
// ---------------------------------------------------------------------------

/// A marca curta da busca: o padrão e as pastas, sem a forma de mostrar
/// (`-i`, o modo de saída, os filtros). Quem repete a busca com outra forma
/// para ver a lista inteira também passa.
fn key_of(search: &Search<'_>) -> String {
    let mut text = String::new();
    for pattern in search.patterns {
        text.push_str(pattern);
        text.push('\u{1}');
    }
    for folder in search.folders {
        text.push('\u{2}');
        text.push_str(&folder.tree.to_string_lossy());
        text.push(':');
        text.push_str(&folder.rel);
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// `true` quando a busca de marca `key` já foi respondida nesta sessão.
fn remembers(memory: &Path, key: &str) -> bool {
    std::fs::read_to_string(memory).is_ok_and(|text| text.lines().any(|line| line == key))
}

/// Grava a busca de marca `key` no estado da sessão. `None` quando a
/// gravação falha: sem ela a busca repetida seria recusada de novo, e quem
/// chama deixa a busca comum passar. Sem estado (`None`), nada se grava.
fn remember(memory: Option<&Path>, key: &str) -> Option<()> {
    let Some(memory) = memory else {
        return Some(());
    };
    let mut kept: Vec<String> = std::fs::read_to_string(memory).map(|text| text.lines().map(str::to_string).collect()).unwrap_or_default();
    kept.push(key.to_string());
    let from = kept.len().saturating_sub(REMEMBERED);
    if let Some(parent) = memory.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    fs::write_atomic(memory, format!("{}\n", kept[from..].join("\n")).as_bytes()).ok()
}

// ---------------------------------------------------------------------------
// A busca nos arquivos
// ---------------------------------------------------------------------------

/// Os lugares que a busca achou num arquivo: as linhas, contadas de 1.
struct FileHits {
    path: String,
    lines: Vec<u64>,
}

/// `true` quando o arquivo não entra na busca: o de configuração com a chave,
/// os de segredo, os de ambiente e as pastas de artefato.
fn skipped(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name == CONFIG_FILE || name.starts_with(".env") || sensitive_pattern(rel).is_some() || is_artifact(rel)
}

/// Onde o padrão casa nos arquivos que o git conhece (os do índice e os novos
/// que ele não ignora) sob as pastas `rels` da árvore `tree`. `None` quando o
/// git falha ou um filtro não se entende. O motor histórico só roda nos
/// testes e lê tudo, sem prazo: a máquina carregada não muda o que ele acha.
fn scan(tree: &Path, rels: &[String], search: &Search<'_>, regex: &Regex) -> Option<Vec<FileHits>> {
    let mut args = vec!["-c", "core.quotePath=false", "ls-files", "-co", "--exclude-standard", "-z", "--"];
    args.extend(rels.iter().map(|rel| if rel.is_empty() { "." } else { rel.as_str() }));
    let listed = git::run(tree, &args);
    if !listed.ok {
        return None;
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let mut found = Vec::new();
    for rel in listed.stdout.split('\0').filter(|rel| !rel.is_empty()) {
        if !seen.insert(rel) {
            continue;
        }
        let hidden = matches!(search.walk, Walk::Rg { .. }) && rel.split('/').any(|part| part.starts_with('.'));
        if hidden || skipped(rel) || !admitted(rel, search.filters, search.walk, rels)? {
            continue;
        }
        let path = tree.join(rel);
        if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_FILE_BYTES) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if bytes.iter().take(8000).any(|byte| *byte == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        if !regex.is_match(&text) {
            continue;
        }
        let lines: Vec<u64> = text.lines().enumerate().filter(|(_, line)| regex.is_match(line)).map(|(at, _)| at as u64 + 1).collect();
        if !lines.is_empty() {
            found.push(FileHits { path: rel.to_string(), lines });
        }
    }
    Some(found)
}

// ---------------------------------------------------------------------------
// A resposta
// ---------------------------------------------------------------------------

/// Um arquivo como a resposta o lê: o texto de agora, as partes do mapa nas
/// linhas dele e se ele mudou depois da passada do mapa.
struct View {
    text: String,
    parts: Option<FileParts>,
    changed: bool,
}

/// O arquivo `rel` da árvore `tree`, com as partes do mapa. O conteúdo que
/// difere do que o mapa leu marca o arquivo como mudado, e as linhas do mapa
/// se levam para as do texto de agora; sem o texto antigo, o arquivo mudado
/// fica sem partes.
fn view_of(scene: &Scene<'_>, tree: &Path, rel: &str) -> Option<View> {
    let text = String::from_utf8_lossy(&std::fs::read(tree.join(rel)).ok()?).into_owned();
    let known = store::read_for_at(scene.model, Need::Parts(rel)).ok().and_then(|map| project_map::parts(&map, rel).ok());
    let Some(parts) = known else {
        return Some(View { text, parts: None, changed: false });
    };
    let read = store::blobs_of(scene.model, &[rel]).ok().and_then(|blobs| blobs.get(rel).cloned()).unwrap_or_default();
    let now = git::run(tree, &["hash-object", "--", rel]).out().unwrap_or_default();
    if read.is_empty() || now.is_empty() || read == now {
        return Some(View { text, parts: Some(parts), changed: false });
    }
    let old = git::run(tree, &["cat-file", "blob", &read]);
    let parts = old.ok.then(|| parts_in_copy(parts, &old.stdout, &text));
    Some(View { text, parts, changed: true })
}

/// A parte mais funda que traz a linha `line`.
fn innermost(parts: &[FilePart], line: u64) -> Option<usize> {
    parts
        .iter()
        .enumerate()
        .filter(|(_, part)| part.line <= line && line <= part.end_line)
        .min_by_key(|(_, part)| (part.end_line.saturating_sub(part.line), u64::MAX - part.line))
        .map(|(at, _)| at)
}

/// Uma linha da resposta de um arquivo: onde começa, o texto e quantos
/// lugares ela cobre.
struct Entry {
    line: u64,
    text: String,
    places: usize,
    /// O começo e o fim da função; `None` nas linhas soltas.
    span: Option<(u64, u64)>,
    /// As linhas numeradas da função, cada uma numa linha da resposta, sob a
    /// entrada dela; vazio em toda entrada que não é a primeira função da
    /// resposta.
    body: String,
}

/// As entradas do arquivo `file`: cada função com as linhas achadas nela e
/// cada linha solta, em ordem de linha, até [`ENTRIES_PER_FILE`]; e quantos
/// lugares ficaram de fora. A primeira função de toda a resposta traz o código
/// dela (até [`SNIPPET_LINES_PER_FN`] linhas): `code_given` diz se alguma
/// função de um arquivo anterior já o trouxe, e passa a dizer que sim quando
/// este arquivo o traz. `None` no arquivo que não se lê mais.
fn entries_of(scene: &Scene<'_>, tree: &Path, file: &FileHits, code_given: &mut bool) -> Option<(Vec<Entry>, usize, bool)> {
    let view = view_of(scene, tree, &file.path)?;
    let mut inside: BTreeMap<usize, Vec<u64>> = BTreeMap::new();
    let mut loose: Vec<u64> = Vec::new();
    for &line in &file.lines {
        match view.parts.as_ref().and_then(|found| innermost(&found.parts, line)) {
            Some(at) => inside.entry(at).or_default().push(line),
            None => loose.push(line),
        }
    }
    let mut entries: Vec<Entry> = Vec::new();
    if let Some(found) = &view.parts {
        for (at, lines) in inside {
            let part = &found.parts[at];
            let shown: Vec<String> = lines.iter().take(HITS_SHOWN).map(u64::to_string).collect();
            let more = if lines.len() > HITS_SHOWN { ", …" } else { "" };
            let text = format!("{}-{} {} ({}{more})", part.line, part.end_line, part.name, shown.join(", "));
            let span = Some((part.line, part.end_line));
            entries.push(Entry { line: part.line, text, places: lines.len(), span, body: String::new() });
        }
    }
    let rows: Vec<&str> = if loose.is_empty() { Vec::new() } else { view.text.lines().collect() };
    for line in loose {
        let row = rows.get(usize::try_from(line).unwrap_or(usize::MAX).saturating_sub(1)).copied().unwrap_or_default();
        let snippet: String = row.trim().chars().take(LOOSE_WIDTH).collect();
        entries.push(Entry { line, text: format!("{line}: {snippet}"), places: 1, span: None, body: String::new() });
    }
    entries.sort_by_key(|entry| entry.line);
    let dropped: usize = entries.iter().skip(ENTRIES_PER_FILE).map(|entry| entry.places).sum();
    entries.truncate(ENTRIES_PER_FILE);
    if !*code_given
        && let Some(first) = entries.iter_mut().find(|entry| entry.span.is_some())
        && let Some(span) = first.span
    {
        first.body = numbered_body(scene.lang, &view.text, span);
        *code_given = true;
    }
    Some((entries, dropped, view.changed))
}

/// O código da função que vai da linha `first` à `last` de `text`: cada linha
/// numerada, numa linha da resposta recuada sob a entrada, até
/// [`SNIPPET_LINES_PER_FN`]; a mais longa é cortada, com a contagem do que
/// ficou de fora. Vazio quando o arquivo não tem essas linhas.
fn numbered_body(lang: Locale, text: &str, (first, last): (u64, u64)) -> String {
    let first = first.max(1);
    let cut = project_map::lines_of(text, first, last);
    let rows: Vec<&str> = cut.lines().collect();
    let mut body = String::new();
    for (at, row) in rows.iter().take(SNIPPET_LINES_PER_FN).enumerate() {
        let _ = write!(body, "\n    {} | {row}", first + at as u64);
    }
    if rows.len() > SNIPPET_LINES_PER_FN {
        let left = (rows.len() - SNIPPET_LINES_PER_FN).to_string();
        body.push_str("\n    ");
        body.push_str(&say("map.answer.more_lines", lang, &[("{count}", &left)]));
    }
    body
}

/// A frase da marca, com as palavras que a triagem achou ou as que faltam.
fn header(mark: Mark, triaged: &Triaged, lang: Locale) -> String {
    let quoted = |words: &[String]| words.iter().map(|word| format!("\"{word}\"")).collect::<Vec<_>>().join(", ");
    match mark {
        Mark::Pinned => {
            // Cravado não exige todas as palavras: o texto cita as que o
            // primeiro achado traz em campo forte, e só sem nenhuma delas cita
            // as da pergunta inteira.
            let found: Vec<String> = triaged.words.iter().filter(|word| !triaged.missing.contains(word)).cloned().collect();
            let shown = if found.is_empty() { &triaged.words } else { &found };
            say("map.answer.pinned", lang, &[("{words}", &quoted(shown))])
        }
        Mark::Partial if !triaged.missing.is_empty() => say("map.answer.partial", lang, &[("{missing}", &quoted(&triaged.missing))]),
        _ => say("map.answer.partial_unsure", lang, &[]),
    }
}

/// A linha da marca para a busca que só lista nomes ou conta: a busca comum
/// roda, e a linha diz a marca e que a resposta por função vem na busca que
/// mostra linhas.
fn names_line(mark: Mark, triaged: &Triaged, lang: Locale) -> String {
    format!("{} {}", header(mark, triaged, lang), say("map.answer.names_only", lang, &[]))
}

/// A resposta com as peças que o filtro entregou: só elas, na ordem da chance
/// e agrupadas por arquivo (o do primeiro colocado vem primeiro), cada uma com
/// o começo, o fim, o nome e, entre parênteses, as linhas achadas dentro dela.
/// A peça sem linha achada vem sem parênteses, e o que a busca achou fora das
/// peças vira a contagem do que ficou de fora. Abre com a frase da marca,
/// `head`, e fecha com a linha de usar as ferramentas de sempre, se o lugar
/// não for este.
fn compose_delivered(scene: &Scene<'_>, tree: &Path, (mark, head): (Mark, &str), hits: &[FileHits], delivered: &[Piece]) -> String {
    let mut paths: Vec<&str> = Vec::new();
    for piece in delivered {
        if !paths.contains(&piece.path.as_str()) {
            paths.push(&piece.path);
        }
    }
    let mut out = head.to_string();
    out.push('\n');
    out.push_str(&say("map.answer.lines", scene.lang, &[]));
    let mut places: usize = hits.iter().filter(|file| !paths.contains(&file.path.as_str())).map(|file| file.lines.len()).sum();
    let mut cut_files = hits.iter().filter(|file| !paths.contains(&file.path.as_str())).count();
    for path in paths {
        let found = hits.iter().find(|file| file.path == path);
        let view = found.and_then(|file| view_of(scene, tree, &file.path));
        out.push('\n');
        out.push_str(path);
        if view.as_ref().is_some_and(|view| view.changed) {
            out.push_str(" (");
            out.push_str(&say("map.answer.changed", scene.lang, &[]));
            out.push(')');
        }
        let mut covered: HashSet<u64> = HashSet::new();
        for piece in delivered.iter().filter(|piece| piece.path == path) {
            let (line, end_line) = view.as_ref().and_then(|view| moved(view, piece)).unwrap_or((u64::from(piece.line), u64::from(piece.end_line)));
            let inside: Vec<u64> = found.map(|file| file.lines.iter().copied().filter(|at| (line..=end_line).contains(at)).collect()).unwrap_or_default();
            covered.extend(&inside);
            let _ = write!(out, "\n  {line}-{end_line} {}", piece.name);
            if !inside.is_empty() {
                let shown: Vec<String> = inside.iter().take(HITS_SHOWN).map(u64::to_string).collect();
                let more = if inside.len() > HITS_SHOWN { ", …" } else { "" };
                let _ = write!(out, " ({}{more})", shown.join(", "));
            }
        }
        let outside = found.map_or(0, |file| file.lines.iter().filter(|at| !covered.contains(at)).count());
        if outside > 0 {
            places += outside;
            cut_files += 1;
        }
    }
    if places > 0 {
        out.push('\n');
        out.push_str(&rest_line(mark, (places, cut_files), scene.lang));
    }
    out.push('\n');
    out.push_str(&say("map.search.use_tools", scene.lang, &[]));
    out
}

/// A linha do que o corte deixou de fora: a resposta cravada ocupa o lugar da
/// busca, e quem a quer inteira a repete; a parcial vem ao lado do resultado
/// da busca, que já traz a lista inteira.
fn rest_line(mark: Mark, (places, files): (usize, usize), lang: Locale) -> String {
    let key = if mark == Mark::Pinned { "map.answer.rest" } else { "map.answer.rest_beside" };
    say(key, lang, &[("{places}", &places.to_string()), ("{files}", &files.to_string())])
}

/// As linhas de começo e de fim da peça `piece` no arquivo como está agora: a
/// parte do mapa de mesmo nome e mais perto da linha dela. `None` quando o
/// arquivo mudou e o mapa não sabe mais onde a peça está.
fn moved(view: &View, piece: &Piece) -> Option<(u64, u64)> {
    let found = view.parts.as_ref()?;
    found
        .parts
        .iter()
        .filter(|part| part.name == piece.name)
        .min_by_key(|part| part.line.abs_diff(u64::from(piece.line)))
        .map(|part| (part.line, part.end_line))
}

/// A resposta inteira: a marca, os arquivos da triagem com as funções, as
/// linhas soltas, o código da primeira função, a contagem do que o corte
/// deixou de fora e os avisos `warnings`; com peças entregues pelo filtro, só
/// elas ([`compose_delivered`]), sem o código. As demais funções, do mesmo
/// arquivo ou de outros, vêm só com o começo, o fim, o nome e as linhas
/// achadas. O código da primeira entra até [`SNIPPET_BUDGET`] caracteres, e a
/// resposta inteira fica abaixo de [`ANSWER_LIMIT`]. `None` quando a busca não
/// achou linha e o mapa não aponta arquivo.
fn compose(scene: &Scene<'_>, tree: &Path, (triaged, mark): (&Triaged, Mark), hits: &[FileHits], delivered: &[Piece], warnings: &[String]) -> Option<String> {
    let mut head = header(mark, triaged, scene.lang);
    if mark == Mark::Pinned {
        head.push(' ');
        head.push_str(&say("map.answer.instead", scene.lang, &[]));
    }
    let mut tail = String::new();
    for warning in warnings {
        tail.push('\n');
        tail.push_str(warning);
    }
    let mut text = if delivered.is_empty() {
        compose_found(scene, tree, (triaged, mark, &head), hits, tail.chars().count())?
    } else {
        compose_delivered(scene, tree, (mark, &head), hits, delivered)
    };
    text.push_str(&tail);
    Some(text)
}

/// A resposta da triagem sem peças do filtro: abre com `head` e traz, de cada
/// arquivo mostrado, as entradas dele; só a primeira função traz o código. O
/// código ocupa o que sobra do teto da resposta depois do resto do texto e dos
/// `tail_chars` caracteres dos avisos que vão no fim.
fn compose_found(scene: &Scene<'_>, tree: &Path, (triaged, mark, head): (&Triaged, Mark, &str), hits: &[FileHits], tail_chars: usize) -> Option<String> {
    let rank: HashMap<&str, usize> = triaged.files.iter().enumerate().map(|(at, file)| (file.path.as_str(), at)).collect();
    let mut ordered: Vec<&FileHits> = hits.iter().collect();
    ordered.sort_by(|a, b| {
        let of = |file: &FileHits| rank.get(file.path.as_str()).copied().unwrap_or(usize::MAX);
        of(a).cmp(&of(b)).then(b.lines.len().cmp(&a.lines.len())).then_with(|| a.path.cmp(&b.path))
    });
    let (shown, left) = ordered.split_at(ordered.len().min(SHOWN_FILES));
    let mut out = head.to_string();
    if shown.is_empty() {
        let files: Vec<String> = triaged.files.iter().take(SHOWN_FILES).map(|file| format!("`{}`", file.path)).collect();
        if files.is_empty() {
            return None;
        }
        out.push('\n');
        out.push_str(&say("map.answer.map_only", scene.lang, &[("{files}", &files.join(", "))]));
        return Some(out);
    }
    let mut places: usize = left.iter().map(|file| file.lines.len()).sum();
    let mut cut_files = left.len();
    let mut blocks: Vec<(&str, bool, Vec<Entry>)> = Vec::new();
    let mut code_given = false;
    for file in shown {
        let Some((entries, dropped, changed)) = entries_of(scene, tree, file, &mut code_given) else {
            places += file.lines.len();
            cut_files += 1;
            continue;
        };
        if dropped > 0 {
            places += dropped;
            cut_files += 1;
        }
        blocks.push((file.path.as_str(), changed, entries));
    }
    // O texto com `room` caracteres para o código da primeira função: ele
    // entra se couber, e senão essa função vem só com a linha dela.
    let render = |room: usize| -> String {
        let mut text = out.clone();
        text.push('\n');
        text.push_str(&say("map.answer.lines_code", scene.lang, &[]));
        for (path, changed, entries) in &blocks {
            text.push('\n');
            text.push_str(path);
            if *changed {
                text.push_str(" (");
                text.push_str(&say("map.answer.changed", scene.lang, &[]));
                text.push(')');
            }
            for entry in entries {
                text.push_str("\n  ");
                text.push_str(&entry.text);
                if entry.body.chars().count() <= room {
                    text.push_str(&entry.body);
                }
            }
        }
        if places > 0 {
            text.push('\n');
            text.push_str(&rest_line(mark, (places, cut_files), scene.lang));
        }
        text
    };
    let bare = render(0).chars().count() + tail_chars;
    Some(render(SNIPPET_BUDGET.min(ANSWER_LIMIT.saturating_sub(bare))))
}

/// As palavras que a triagem lê no padrão da busca.
use super::words;

/// A régua do gasto: o que o Claude recebe e quanto gasta, com as buscas reais.
#[cfg(test)]
#[path = "ruler.rs"]
pub(crate) mod ruler;

/// A busca numa pasta ou num tipo de arquivo só manda ao filtro, e só devolve,
/// o que está nela.
#[cfg(test)]
#[path = "scoped.rs"]
pub(crate) mod scoped;

/// O projeto de teste da busca por palavra: um repositório git com fontes
/// que têm funções conhecidas do mapa, e o mapa com o blob do que cada
/// arquivo tinha ao ser lido.
#[cfg(test)]
pub(crate) mod fixture {
    use std::path::Path;
    #[cfg(test)]
    use std::path::PathBuf;
    use std::process::Command;

    use mustard_core::ProjectConfig;
    use mustard_core::domain::map_filter::{FilterError, FilterRequest, FilterUsage, Filtered, MapFilter, Scored, judged};

    use crate::shared::search_door::Assembled;

    /// O arquivo do frete: `calcular_frete` nas linhas 2 a 6 (a palavra
    /// `imposto` só aparece num comentário) e `desconto_frete` de 8 a 10.
    pub(crate) const FREIGHT: &str = "// Frete do pedido.\npub fn calcular_frete(peso: u32) -> u32 {\n    // imposto embutido\n    let base = peso * 2;\n    base + 10\n}\n\npub fn desconto_frete(total: u32) -> u32 {\n    total / 10\n}\n";

    /// O arquivo do pedido: `fechar_pedido` nas linhas 1 a 4, que chama o
    /// frete na linha 2.
    pub(crate) const ORDER: &str = "pub fn fechar_pedido(peso: u32) -> u32 {\n    let frete = calcular_frete(peso);\n    frete + 1\n}\n";

    /// A nota fora do mapa, com o nome e a palavra `imposto` na linha 1.
    pub(crate) const NOTES: &str = "O calcular_frete soma o imposto.\n";

    /// Roda o `git` em `dir` com identidade de teste e devolve a saída.
    pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Um projeto com o `mustard.json` `config`, os arquivos `files` (caminho
    /// e texto) commitados num repositório novo, e o mapa `map`, ao qual cada
    /// módulo ganha o blob do arquivo como está em `root`. A raiz vem
    /// resolvida, como a do despachante.
    pub(crate) fn repo_with(config: &str, files: &[(&str, &str)], mut map: serde_json::Value) -> (tempfile::TempDir, PathBuf) {
        // These fixtures exercise an explicitly supplied fake judge. Default
        // production behavior is covered by the native installation proof.
        let mut config: serde_json::Value = serde_json::from_str(config).unwrap();
        config.as_object_mut().unwrap().entry("ai").or_insert(serde_json::json!({"fallback":true}));
        let search = config.as_object_mut().unwrap().entry("search").or_insert(serde_json::json!({}));
        search.as_object_mut().unwrap().entry("filter").or_insert(serde_json::json!("jev"));
        let config = config.to_string();
        let dir = tempfile::tempdir().expect("tempdir");
        let root = PathBuf::from(crate::shared::paths::on_disk(dir.path()));
        let key = format!("word_search.repo_with:{config}:{files:?}");
        crate::shared::test_fixture::repo_from_template(&root, &key, |root| {
            std::fs::write(root.join("mustard.json"), &config).expect("config");
            for (rel, text) in files {
                let path = root.join(rel);
                std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
                std::fs::write(path, text).expect("file");
            }
            git(root, &["init", "-q", "-b", "dev"]);
            git(root, &["add", "-A"]);
            git(root, &["commit", "-q", "-m", "semente"]);
        });
        for module in map["modules"].as_array_mut().expect("modules") {
            let rel = module["path"].as_str().expect("path").to_string();
            module["blob"] = serde_json::json!(git(&root, &["hash-object", "--", &rel]));
        }
        mustard_core::io::project_map::write_text(&root, &map.to_string()).expect("map");
        (dir, root)
    }

    /// O projeto do frete: os dois fontes e a nota, com o mapa dos fontes.
    pub(crate) fn repo(config: &str) -> (tempfile::TempDir, PathBuf) {
        let map = serde_json::json!({ "modules": [
            { "path": "src/frete.rs", "language": "rust", "loc": 10, "declarations": [
                { "kind": "function", "name": "calcular_frete", "line": 2, "end_line": 6,
                  "signature": "pub fn calcular_frete(peso: u32) -> u32", "body_comment": "imposto embutido" },
                { "kind": "function", "name": "desconto_frete", "line": 8, "end_line": 10 }
            ] },
            { "path": "src/pedido.rs", "language": "rust", "loc": 4, "declarations": [
                { "kind": "function", "name": "fechar_pedido", "line": 1, "end_line": 4 }
            ] }
        ] });
        repo_with(config, &[("src/frete.rs", FREIGHT), ("src/pedido.rs", ORDER), ("docs/notas.md", NOTES)], map)
    }

    /// A montagem do filtro que um teste põe no lugar do Jev.
    pub(crate) type TestAssemble = std::rc::Rc<dyn Fn(&Path, &ProjectConfig) -> Result<Assembled, FilterError>>;

    thread_local! {
        /// O filtro que o gancho monta nesta linha de execução no lugar do Jev.
        static FILTER: std::cell::RefCell<Option<TestAssemble>> = const { std::cell::RefCell::new(None) };
    }

    /// Roda `run` com o filtro que `assemble` monta no lugar do Jev em todo
    /// gancho de busca desta linha de execução, e o tira ao fim, também quando
    /// `run` falha.
    pub(crate) fn with_filter<T>(assemble: TestAssemble, run: impl FnOnce() -> T) -> T {
        struct Leaves;
        impl Drop for Leaves {
            fn drop(&mut self) {
                FILTER.with(|cell| *cell.borrow_mut() = None);
            }
        }
        FILTER.with(|cell| *cell.borrow_mut() = Some(assemble));
        let _leaves = Leaves;
        run()
    }

    /// Um filtro de mentira: guarda cada pedido e dá a chance de cada
    /// candidato pelo nome (`chances`, e 0,01 para os outros), com a chance
    /// de algum candidato servir (`exists`); ou falha com `error`.
    #[derive(Clone)]
    pub(crate) struct Judge {
        asked: std::rc::Rc<std::cell::RefCell<Vec<FilterRequest>>>,
        chances: Vec<(&'static str, f64)>,
        exists: f64,
        error: Option<FilterError>,
        usage: FilterUsage,
    }

    impl MapFilter for Judge {
        fn filter(&self, request: &FilterRequest) -> Result<Filtered, FilterError> {
            self.asked.borrow_mut().push(request.clone());
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            let scores: Vec<Scored> = request
                .candidates
                .iter()
                .map(|candidate| {
                    let chance = self.chances.iter().find(|(name, _)| *name == candidate.name).map_or(0.01, |(_, chance)| *chance);
                    Scored { id: candidate.id, score: chance }
                })
                .collect();
            let (verdict, kept) = judged(&scores, self.exists, request.cut);
            Ok(Filtered { verdict, kept, usage: self.usage.clone() })
        }
    }

    impl Judge {
        /// O filtro seguro: dá `chances` e quase certeza de que algum candidato
        /// serve.
        pub(crate) fn sure_of(chances: &[(&'static str, f64)]) -> Self {
            Self { asked: std::rc::Rc::default(), chances: chances.to_vec(), exists: 0.99, error: None, usage: FilterUsage::default() }
        }

        /// A montagem que entrega este filtro, como a chave no projeto.
        pub(crate) fn assemble(&self) -> impl Fn(&Path, &ProjectConfig) -> Result<Assembled, FilterError> + '_ {
            move |_, _| Ok(Assembled { name: "jev", filter: Box::new(self.clone()), warning: None })
        }

        /// Roda `run` com este filtro no lugar do Jev em todo gancho de busca
        /// desta linha de execução: o gancho inteiro, sem rede e sem chave.
        pub(crate) fn installed<T>(&self, run: impl FnOnce() -> T) -> T {
            let judge = self.clone();
            with_filter(std::rc::Rc::new(move |_: &Path, _: &ProjectConfig| Ok(Assembled { name: "jev", filter: Box::new(judge.clone()), warning: None })), run)
        }

        pub(crate) fn calls(&self) -> usize {
            self.asked.borrow().len()
        }

        pub(crate) fn last(&self) -> FilterRequest {
            self.asked.borrow().last().cloned().expect("the filter was called")
        }
    }
}
