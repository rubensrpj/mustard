//! `word_search` — a resposta do mapa no lugar da busca por palavra.
//!
//! O `Grep` do Claude e o `grep`/`rg` do terminal, numa pasta de código do
//! projeto, recebem uma de três marcas, dadas pela triagem do mapa:
//!
//! - **cravado** — o primeiro arquivo tem a nota mais alta e a chance do
//!   corte medido na régua, mesmo com palavras da busca fora dos campos
//!   fortes dele: a resposta cita só as que ele traz;
//! - **parcial** — o mapa achou parte: os candidatos vão ao filtro (o Jev)
//!   pela porta única da busca ([`crate::shared::search_door`]), e a resposta
//!   traz só as peças que ele entrega; sem chave, com o filtro desligado ou
//!   falhando, vale a resposta da triagem, que diz quais palavras faltam, e
//!   o aviso do motivo sai uma vez por sessão;
//! - **não achou** — o mapa não achou nada, ou o filtro disse que nenhum
//!   candidato serve: a busca comum roda, e uma linha diz as palavras já
//!   quebradas e a próxima busca, exata.
//!
//! No cravado e no parcial, a busca que mostra linhas recebe a resposta no
//! lugar da busca comum. O cravado responde da triagem, sem chamar o filtro. A que só lista nomes de arquivo ou conta (`-l`, `-c`)
//! roda como veio, com uma linha só da marca, e não fica guardada como
//! respondida: a que mostra linhas depois ainda recebe a resposta. A resposta roda a
//! mesma busca (mesmo padrão, mesmas pastas) nos arquivos que o git conhece e
//! agrupa o que achou pelas funções do mapa: o arquivo, a linha de começo e a
//! de fim de cada função, com as linhas achadas dentro dela. A linha fora de
//! função conhecida (json, markdown) vem solta. Os arquivos saem na ordem da
//! triagem e ficam só os primeiros; o resto vira uma linha com a contagem de
//! lugares e de arquivos.
//!
//! A mesma busca repetida na sessão passa: o padrão e a pasta respondidos
//! ficam num estado curto da sessão. O arquivo que mudou depois da passada
//! do mapa (o conteúdo difere do que o mapa leu) é relido só para esta
//! resposta, e as linhas do mapa se levam para as dele; o arquivo vem
//! marcado como mudado. Numa cópia de trabalho, a árvore lida é a da cópia.
//!
//! Nunca falha: sem mapa, sem sessão, com regex que esta leitura não entende
//! ou passando do tempo, a resposta é passar, e a busca comum segue.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mustard_core::domain::map_filter::Verdict;
use mustard_core::domain::model::contract::{Ctx, HookInput};
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::project_map::{self, FilePart, FileParts};
use mustard_core::domain::triage::{not_found, Mark};
use mustard_core::io::fs;
use mustard_core::io::map_triage::{self, Triaged};
use mustard_core::io::project_map::{self as store, Need};
use mustard_core::platform::git;
use mustard_core::platform::i18n::Locale;
use mustard_core::{ClaudePaths, ProjectConfig};
use regex::{Regex, RegexBuilder};
use serde_json::json;

use crate::commands::map::Assemble;
use crate::shared::code_route::{holds_code, parts_in_copy, ProjectPath};
use crate::shared::config_key::{takes, NameFilter, Walk, CONFIG_FILE};
use crate::shared::paths::{is_artifact, sensitive_pattern};
use crate::shared::say::say;
use crate::shared::search_door::{self as door, Ask, Numbers, Outcome, Piece};

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

/// Quantas palavras da busca vão à triagem.
const MAX_WORDS: usize = 12;

/// O tempo que a busca própria tem: passando dele, a busca comum roda.
const BUDGET: Duration = Duration::from_millis(800);

/// O maior arquivo lido, em bytes; o maior que isso é dado, não código.
const MAX_FILE_BYTES: u64 = 1_000_000;

/// Quantas buscas o estado da sessão guarda.
const REMEMBERED: usize = 200;

/// O nome do estado da sessão que guarda as buscas já respondidas.
const STATE_FILE: &str = "word-searches";

/// O que o gancho diz da busca.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reply {
    /// Nada a dizer: a busca comum roda como veio.
    Pass,
    /// A busca comum roda, com esta linha junto.
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
    /// A expressão estendida do `grep -E`.
    Extended,
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
}

/// O que o gancho diz da busca `search`: a resposta no lugar dela, a linha
/// que vai junto dela ou nada.
pub(crate) fn reply(scene: &Scene<'_>, search: &Search<'_>) -> Reply {
    try_reply(scene, search).unwrap_or(Reply::Pass)
}

/// A resposta do gancho à busca `search` de `input`, no projeto `root`: passa
/// com a chave `search.answer` desligada e sem sessão de verdade, que não tem
/// onde guardar a busca respondida.
pub(crate) fn hook_reply(root: &str, input: &HookInput, ctx: &Ctx, search: &Search<'_>) -> Reply {
    if !ctx.config.search_answer() {
        return Reply::Pass;
    }
    let root = Path::new(root);
    let Some(memory) = memory_path(root, input.session_id.as_deref(), input.agent_id.as_deref()) else {
        return Reply::Pass;
    };
    let scene = Scene {
        root,
        model: &store::model_path(root),
        memory: Some(&memory),
        session: input.session_id.as_deref(),
        lang: ctx.config.language().text_or_default(),
        languages: &Languages::of(&ctx.config),
        config: &ctx.config,
        assemble: &crate::commands::map::jev,
    };
    reply(&scene, search)
}

/// O arquivo do estado da sessão `session` do projeto `root`, e o do
/// subagente `agent` dentro dela: cada um repete a busca por conta própria,
/// pois cada um tem a conversa dele. `None` sem sessão de verdade.
pub(crate) fn memory_path(root: &Path, session: Option<&str>, agent: Option<&str>) -> Option<PathBuf> {
    let plain = |text: &str| !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let session = session.map(str::trim).filter(|session| plain(session) && *session != "unknown")?;
    let name = match agent.map(str::trim).filter(|agent| !agent.is_empty()) {
        Some(agent) if plain(agent) => format!("{STATE_FILE}-{agent}"),
        Some(_) => return None,
        None => STATE_FILE.to_string(),
    };
    Some(ClaudePaths::for_project(root).ok()?.claude_dir().join(".session").join(session).join(name))
}

/// Os filtros de nome de arquivo do tipo `kind` do `rg` e da ferramenta de
/// busca; `None` no tipo que esta leitura não conhece.
pub(crate) fn type_filters(kind: &str) -> Option<Vec<NameFilter>> {
    let extensions: &[&str] = match kind {
        "rust" | "rs" => &["rs"],
        "js" | "javascript" => &["js", "jsx", "mjs", "cjs"],
        "ts" | "typescript" => &["ts", "tsx", "mts", "cts"],
        "py" | "python" => &["py"],
        "go" => &["go"],
        "java" => &["java"],
        "kotlin" | "kt" => &["kt", "kts"],
        "cs" | "csharp" => &["cs"],
        "cpp" => &["cpp", "cc", "cxx", "hpp", "hh", "h"],
        "c" => &["c", "h"],
        "php" => &["php"],
        "ruby" | "rb" => &["rb"],
        "swift" => &["swift"],
        "md" | "markdown" => &["md", "markdown"],
        "json" => &["json"],
        "yaml" => &["yml", "yaml"],
        "toml" => &["toml"],
        "html" => &["html", "htm"],
        "css" => &["css", "scss"],
        "sql" => &["sql"],
        "sh" => &["sh", "bash"],
        _ => return None,
    };
    Some(extensions.iter().map(|ext| NameFilter { exclude: false, glob: format!("*.{ext}") }).collect())
}

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
    let triaged = map_triage::triage_at(scene.model, (&question, ""), scene.languages, RANKED_FILES).ok()?;
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
    // Só o parcial vai ao filtro: o cravado responde da triagem.
    let mut warnings: Vec<String> = Vec::new();
    let judged = if mark == Mark::Partial { judge(scene, &question, &triaged, &mut warnings) } else { Judged::Triage };
    if judged == Judged::NotFound {
        remember(scene.memory, &key)?;
        return Some(Reply::Note(not_found(&question, &triaged.words, scene.lang)));
    }
    let delivered = match &judged {
        Judged::Pieces(pieces) => pieces.as_slice(),
        _ => &[],
    };
    let mut text = compose(scene, tree, &triaged, mark, &hits, delivered)?;
    for warning in warnings {
        text.push('\n');
        text.push_str(&warning);
    }
    remember(scene.memory, &key)?;
    Some(Reply::Answer(text))
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

/// A busca parcial pela porta única: as palavras são o pedido, e num projeto
/// com texto e código em línguas diferentes vão também como a frase, na
/// língua do texto, para o filtro ler a palavra como ela é e não como pedaço
/// de nome. O motivo de não haver filtro, ou de ele falhar, entra em
/// `warnings`, uma vez por sessão.
fn judge(scene: &Scene<'_>, question: &str, triaged: &Triaged, warnings: &mut Vec<String>) -> Judged {
    let (root, session, lang) = (scene.root, scene.session, scene.lang);
    let started = Instant::now();
    let Some(assembled) = door::chosen_filter(root, session, lang, scene.config, scene.assemble, warnings) else {
        return Judged::Triage;
    };
    let numbers = Numbers::read(root, session, lang, scene.config, warnings);
    let intent = if scene.languages.codes().len() > 1 { question } else { "" };
    let ask = Ask { root, query: question, intent, lang, languages: scene.languages, numbers: &numbers, triaged };
    let Ok(classified) = door::classify(&ask, &assembled) else { return Judged::Triage };
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
    let _ = crate::commands::spec_events::conversation::record_measured_call(
        root,
        "word search",
        None,
        session,
        started,
        &json!({ "ok": true }),
        classified.measured,
    );
    judged
}

// ---------------------------------------------------------------------------
// As palavras e o padrão
// ---------------------------------------------------------------------------

/// As palavras que a triagem do mapa lê no padrão: os trechos de letras,
/// números e sublinhado com ao menos dois caracteres, sem os operadores da
/// expressão (`\w`, `\(`, `[a-z]`, `{2,3}`), na ordem e sem repetir. Em texto
/// fixo, tudo é palavra.
pub(crate) fn words_of(patterns: &[String], fixed: bool) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for pattern in patterns {
        let mut word = String::new();
        let mut chars = pattern.chars();
        let mut flush = |word: &mut String| {
            if word.chars().count() >= 2 && !words.contains(word) {
                words.push(std::mem::take(word));
            } else {
                word.clear();
            }
        };
        while let Some(c) = chars.next() {
            match c {
                '\\' if !fixed => {
                    chars.next();
                    flush(&mut word);
                }
                '[' if !fixed => {
                    flush(&mut word);
                    skip_class(&mut chars);
                }
                '{' if !fixed => {
                    flush(&mut word);
                    for skipped in chars.by_ref() {
                        if skipped == '}' {
                            break;
                        }
                    }
                }
                c if c.is_alphanumeric() || c == '_' => word.push(c),
                _ => flush(&mut word),
            }
        }
        flush(&mut word);
    }
    words.truncate(MAX_WORDS);
    words
}

/// Pula o resto de uma classe de caracteres (`[a-z]`, `[^x]`, `[[:alpha:]]`)
/// cujo `[` já foi lido.
fn skip_class(chars: &mut std::str::Chars<'_>) {
    let mut rest = chars.clone().peekable();
    if rest.peek() == Some(&'^') {
        rest.next();
        chars.next();
    }
    if rest.peek() == Some(&']') {
        rest.next();
        chars.next();
    }
    while let Some(c) = chars.next() {
        match c {
            '[' if chars.clone().next() == Some(':') => {
                let mut previous = ' ';
                for inner in chars.by_ref() {
                    if previous == ':' && inner == ']' {
                        break;
                    }
                    previous = inner;
                }
            }
            ']' => return,
            _ => {}
        }
    }
}

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
            Dialect::Rust | Dialect::Extended => pattern.clone(),
        })
        .map(|source| format!("(?:{source})"))
        .collect();
    let mut source = parts.join("|");
    if search.whole_word {
        source = format!(r"\b(?:{source})\b");
    }
    RegexBuilder::new(&source)
        .multi_line(true)
        .case_insensitive(search.ignore_case)
        .size_limit(1 << 20)
        .dfa_size_limit(4 << 20)
        .build()
        .ok()
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
    let Some(memory) = memory else { return Some(()) };
    let mut kept: Vec<String> = std::fs::read_to_string(memory)
        .map(|text| text.lines().map(str::to_string).collect())
        .unwrap_or_default();
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

/// Se os filtros de nome `filters` deixam o arquivo `rel` na busca: o último
/// que casa com o nome decide, e sem nenhum que case, o arquivo entra, salvo
/// quando há filtro de entrada (no `grep`, quando o primeiro é de entrada).
/// `None` quando algum filtro usa o que esta leitura não entende.
fn admitted(rel: &str, filters: &[NameFilter], walk: Walk) -> Option<bool> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let braces = walk != Walk::Grep;
    for filter in filters.iter().rev() {
        if takes(&filter.glob, name, braces)? {
            return Some(!filter.exclude);
        }
    }
    Some(match walk {
        Walk::Grep => filters.first().is_none_or(|filter| filter.exclude),
        Walk::Rg { .. } => filters.iter().all(|filter| filter.exclude),
    })
}

/// Onde o padrão casa nos arquivos que o git conhece (os do índice e os novos
/// que ele não ignora) sob as pastas `rels` da árvore `tree`. `None` quando o
/// git falha, um filtro não se entende ou o tempo acaba.
fn scan(tree: &Path, rels: &[String], search: &Search<'_>, regex: &Regex) -> Option<Vec<FileHits>> {
    let started = Instant::now();
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
        if started.elapsed() > BUDGET {
            return None;
        }
        let hidden = matches!(search.walk, Walk::Rg { .. }) && rel.split('/').any(|part| part.starts_with('.'));
        if hidden || skipped(rel) || !admitted(rel, search.filters, search.walk)? {
            continue;
        }
        let path = tree.join(rel);
        if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_FILE_BYTES) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if bytes.iter().take(8000).any(|byte| *byte == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        if !regex.is_match(&text) {
            continue;
        }
        let lines: Vec<u64> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| regex.is_match(line))
            .map(|(at, _)| at as u64 + 1)
            .collect();
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
    let known = store::read_for_at(scene.model, Need::Parts(rel))
        .ok()
        .and_then(|map| project_map::parts(&map, rel).ok());
    let Some(parts) = known else { return Some(View { text, parts: None, changed: false }) };
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
}

/// As entradas do arquivo `file`: cada função com as linhas achadas nela e
/// cada linha solta, em ordem de linha, até [`ENTRIES_PER_FILE`]; e quantos
/// lugares ficaram de fora. `None` no arquivo que não se lê mais.
fn entries_of(scene: &Scene<'_>, tree: &Path, file: &FileHits) -> Option<(Vec<Entry>, usize, bool)> {
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
            entries.push(Entry { line: part.line, text, places: lines.len() });
        }
    }
    let rows: Vec<&str> = if loose.is_empty() { Vec::new() } else { view.text.lines().collect() };
    for line in loose {
        let row = rows.get(usize::try_from(line).unwrap_or(usize::MAX).saturating_sub(1)).copied().unwrap_or_default();
        let snippet: String = row.trim().chars().take(LOOSE_WIDTH).collect();
        entries.push(Entry { line, text: format!("{line}: {snippet}"), places: 1 });
    }
    entries.sort_by_key(|entry| entry.line);
    let dropped: usize = entries.iter().skip(ENTRIES_PER_FILE).map(|entry| entry.places).sum();
    entries.truncate(ENTRIES_PER_FILE);
    Some((entries, dropped, view.changed))
}

/// A frase da marca, com as palavras que a triagem achou ou as que faltam.
fn header(mark: Mark, triaged: &Triaged, lang: Locale) -> String {
    let quoted = |words: &[String]| words.iter().map(|word| format!("\"{word}\"")).collect::<Vec<_>>().join(", ");
    match mark {
        Mark::Pinned => {
            // Cravado não exige todas as palavras: o texto cita as que o
            // primeiro achado traz em campo forte, e só sem nenhuma delas cita
            // as da pergunta inteira.
            let found: Vec<String> =
                triaged.words.iter().filter(|word| !triaged.missing.contains(word)).cloned().collect();
            let shown = if found.is_empty() { &triaged.words } else { &found };
            say("map.answer.pinned", lang, &[("{words}", &quoted(shown))])
        }
        Mark::Partial if !triaged.missing.is_empty() => {
            say("map.answer.partial", lang, &[("{missing}", &quoted(&triaged.missing))])
        }
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
/// peças vira a contagem do que ficou de fora. Fecha com a linha de usar as
/// ferramentas de sempre, se o lugar não for este.
fn compose_delivered(scene: &Scene<'_>, tree: &Path, hits: &[FileHits], delivered: &[Piece]) -> String {
    let mut paths: Vec<&str> = Vec::new();
    for piece in delivered {
        if !paths.contains(&piece.path.as_str()) {
            paths.push(&piece.path);
        }
    }
    let mut out = say("map.answer.instead", scene.lang, &[]);
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
            let (line, end_line) = view
                .as_ref()
                .and_then(|view| moved(view, piece))
                .unwrap_or((u64::from(piece.line), u64::from(piece.end_line)));
            let inside: Vec<u64> = found
                .map(|file| file.lines.iter().copied().filter(|at| (line..=end_line).contains(at)).collect())
                .unwrap_or_default();
            covered.extend(&inside);
            out.push_str(&format!("\n  {line}-{end_line} {}", piece.name));
            if !inside.is_empty() {
                let shown: Vec<String> = inside.iter().take(HITS_SHOWN).map(u64::to_string).collect();
                let more = if inside.len() > HITS_SHOWN { ", …" } else { "" };
                out.push_str(&format!(" ({}{more})", shown.join(", ")));
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
        out.push_str(&say(
            "map.answer.rest",
            scene.lang,
            &[("{places}", &places.to_string()), ("{files}", &cut_files.to_string())],
        ));
    }
    out.push('\n');
    out.push_str(&say("map.search.use_tools", scene.lang, &[]));
    out
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

/// A resposta inteira: a marca, os arquivos da triagem com as funções e as
/// linhas soltas, e a contagem do que o corte deixou de fora; com peças
/// entregues pelo filtro, só elas ([`compose_delivered`]). `None` quando a
/// busca não achou linha e o mapa não aponta arquivo.
fn compose(
    scene: &Scene<'_>,
    tree: &Path,
    triaged: &Triaged,
    mark: Mark,
    hits: &[FileHits],
    delivered: &[Piece],
) -> Option<String> {
    if !delivered.is_empty() {
        return Some(compose_delivered(scene, tree, hits, delivered));
    }
    let rank: HashMap<&str, usize> = triaged.files.iter().enumerate().map(|(at, file)| (file.path.as_str(), at)).collect();
    let mut ordered: Vec<&FileHits> = hits.iter().collect();
    ordered.sort_by(|a, b| {
        let of = |file: &FileHits| rank.get(file.path.as_str()).copied().unwrap_or(usize::MAX);
        of(a).cmp(&of(b)).then(b.lines.len().cmp(&a.lines.len())).then_with(|| a.path.cmp(&b.path))
    });
    let (shown, left) = ordered.split_at(ordered.len().min(SHOWN_FILES));
    let mut out = header(mark, triaged, scene.lang);
    if mark == Mark::Pinned {
        out.push(' ');
        out.push_str(&say("map.answer.instead", scene.lang, &[]));
    }
    if shown.is_empty() {
        let files: Vec<String> = triaged.files.iter().take(SHOWN_FILES).map(|file| format!("`{}`", file.path)).collect();
        if files.is_empty() {
            return None;
        }
        out.push('\n');
        out.push_str(&say("map.answer.map_only", scene.lang, &[("{files}", &files.join(", "))]));
        return Some(out);
    }
    out.push('\n');
    out.push_str(&say("map.answer.lines", scene.lang, &[]));
    let mut places: usize = left.iter().map(|file| file.lines.len()).sum();
    let mut cut_files = left.len();
    for file in shown {
        let Some((entries, dropped, changed)) = entries_of(scene, tree, file) else {
            places += file.lines.len();
            cut_files += 1;
            continue;
        };
        out.push('\n');
        out.push_str(&file.path);
        if changed {
            out.push_str(" (");
            out.push_str(&say("map.answer.changed", scene.lang, &[]));
            out.push(')');
        }
        for entry in &entries {
            out.push_str("\n  ");
            out.push_str(&entry.text);
        }
        if dropped > 0 {
            places += dropped;
            cut_files += 1;
        }
    }
    if places > 0 {
        out.push('\n');
        out.push_str(&say(
            "map.answer.rest",
            scene.lang,
            &[("{places}", &places.to_string()), ("{files}", &cut_files.to_string())],
        ));
    }
    Some(out)
}

/// O projeto de teste da busca por palavra: um repositório git com fontes
/// que têm funções conhecidas do mapa, e o mapa com o blob do que cada
/// arquivo tinha ao ser lido.
#[cfg(test)]
pub(crate) mod fixture {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// O arquivo do frete: `calcular_frete` nas linhas 2 a 6 (a palavra
    /// `imposto` só aparece num comentário) e `desconto_frete` de 8 a 10.
    pub(crate) const FRETE: &str = "// Frete do pedido.\npub fn calcular_frete(peso: u32) -> u32 {\n    // imposto embutido\n    let base = peso * 2;\n    base + 10\n}\n\npub fn desconto_frete(total: u32) -> u32 {\n    total / 10\n}\n";

    /// O arquivo do pedido: `fechar_pedido` nas linhas 1 a 4, que chama o
    /// frete na linha 2.
    pub(crate) const PEDIDO: &str = "pub fn fechar_pedido(peso: u32) -> u32 {\n    let frete = calcular_frete(peso);\n    frete + 1\n}\n";

    /// A nota fora do mapa, com o nome e a palavra `imposto` na linha 1.
    pub(crate) const NOTAS: &str = "O calcular_frete soma o imposto.\n";

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
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = std::fs::canonicalize(dir.path()).expect("resolved tempdir");
        let root = PathBuf::from(resolved.to_string_lossy().trim_start_matches(r"\\?\").to_string());
        std::fs::write(root.join("mustard.json"), config).expect("config");
        for (rel, text) in files {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
            std::fs::write(path, text).expect("file");
        }
        git(&root, &["init", "-q", "-b", "dev"]);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "semente"]);
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
        repo_with(config, &[("src/frete.rs", FRETE), ("src/pedido.rs", PEDIDO), ("docs/notas.md", NOTAS)], map)
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{self, git};
    use super::*;
    use crate::commands::map::Assembled;
    use crate::shared::code_route::project_path;
    use mustard_core::domain::map_filter::{
        judged, FilterError, FilterRequest, FilterUsage, Filtered, MapFilter, Scored,
    };
    use mustard_core::domain::triage::Signals;

    fn owned(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    /// A busca do gancho pela árvore `tree`, nos trechos `folders`, que mostra
    /// as linhas.
    fn search_in(root: &Path, tree: &Path, patterns: &[&str], folders: &[&str]) -> Reply {
        search_showing(root, tree, patterns, folders, true)
    }

    /// A montagem sem chave: a busca parcial responde só com a triagem.
    fn without_key(_: &Path, _: &ProjectConfig) -> Result<Assembled, FilterError> {
        Err(FilterError::MissingKey)
    }

    /// A mesma busca, mostrando as linhas achadas ou só listando nomes.
    fn search_showing(root: &Path, tree: &Path, patterns: &[&str], folders: &[&str], shows_lines: bool) -> Reply {
        search_through(root, tree, patterns, folders, shows_lines, &without_key)
    }

    /// A mesma busca, com o filtro que `assemble` monta.
    fn search_through(
        root: &Path,
        tree: &Path,
        patterns: &[&str],
        folders: &[&str],
        shows_lines: bool,
        assemble: &Assemble<'_>,
    ) -> Reply {
        let both = Languages::new(["pt-BR", "en-US"]);
        let rg = (Dialect::Rust, Walk::Rg { unignored: false });
        search_as(root, tree, (patterns, folders, shows_lines), (&both, rg), assemble)
    }

    /// A mesma busca, nas línguas e com o programa dados.
    fn search_as(
        root: &Path,
        tree: &Path,
        (patterns, folders, shows_lines): (&[&str], &[&str], bool),
        (languages, (dialect, walk)): (&Languages, (Dialect, Walk)),
        assemble: &Assemble<'_>,
    ) -> Reply {
        let patterns = owned(patterns);
        let folders: Vec<ProjectPath> = folders
            .iter()
            .map(|folder| project_path(&root.to_string_lossy(), &tree.to_string_lossy(), folder).expect("a project folder"))
            .collect();
        let search = Search {
            patterns: &patterns,
            dialect,
            ignore_case: false,
            whole_word: false,
            folders: &folders,
            filters: &[],
            walk,
            shows_lines,
        };
        let memory = root.join(".claude/.session/teste/word-searches");
        let config = ProjectConfig::load(root);
        let scene = Scene {
            root,
            model: &store::model_path(root),
            memory: Some(&memory),
            session: Some("teste"),
            lang: Locale::PtBr,
            languages,
            config: &config,
            assemble,
        };
        reply(&scene, &search)
    }

    fn answer(reply: Reply) -> String {
        match reply {
            Reply::Answer(text) => text,
            other => panic!("an answer was expected, got {other:?}"),
        }
    }

    #[test]
    fn the_words_come_from_the_pattern_without_the_expression_operators() {
        assert_eq!(words_of(&owned(&["calcular_frete"]), false), ["calcular_frete"]);
        assert_eq!(words_of(&owned(&[r"foo\w+bar"]), false), ["foo", "bar"]);
        assert_eq!(words_of(&owned(&["[a-z]+Total{2,3}"]), false), ["Total"]);
        assert_eq!(words_of(&owned(&["a|imposto|imposto"]), false), ["imposto"], "one letter is no word and a repeat counts once");
        assert_eq!(words_of(&owned(&["foo", "bar baz"]), false), ["foo", "bar", "baz"]);
        assert_eq!(words_of(&owned(&["a.b\\(c"]), true), Vec::<String>::new());
        assert_eq!(words_of(&owned(&["foo\\bar"]), true), ["foo", "bar"], "fixed text has no operator");
        assert_eq!(words_of(&owned(&["ção", "ação"]), false), ["ção", "ação"]);
        let many = (0..20).map(|n| format!("word{n}")).collect::<Vec<_>>();
        assert_eq!(words_of(&many, false).len(), MAX_WORDS);
    }

    #[test]
    fn a_basic_grep_pattern_reads_its_escaped_operators() {
        assert_eq!(basic_to_extended(r"a\|b"), "a|b");
        assert_eq!(basic_to_extended("a|b"), r"a\|b");
        assert_eq!(basic_to_extended(r"\(x\)\{2\}"), "(x){2}");
        assert_eq!(basic_to_extended("f(x)"), r"f\(x\)");
        assert_eq!(basic_to_extended(r"\<word\>"), r"\bword\b");
        assert_eq!(basic_to_extended("[|(]"), "[|(]");
        assert_eq!(basic_to_extended("[^]|]x"), "[^]|]x");
        assert_eq!(basic_to_extended("[[:alpha:]|]"), "[[:alpha:]|]");
    }

    #[test]
    fn the_last_name_filter_that_matches_decides_the_file() {
        let rg = Walk::Rg { unignored: false };
        let only = |globs: &[&str]| globs.iter().map(|glob| NameFilter::rg(glob)).collect::<Vec<_>>();
        assert_eq!(admitted("src/a.rs", &[], rg), Some(true));
        assert_eq!(admitted("src/a.rs", &only(&["*.rs"]), rg), Some(true));
        assert_eq!(admitted("docs/a.md", &only(&["*.rs"]), rg), Some(false), "an input filter leaves the rest out");
        assert_eq!(admitted("docs/a.md", &only(&["!*.rs"]), rg), Some(true));
        assert_eq!(admitted("src/a.rs", &only(&["!*.rs"]), rg), Some(false));
        assert_eq!(admitted("src/a.rs", &only(&["!*.rs", "*.rs"]), rg), Some(true), "the later filter wins");
        assert_eq!(admitted("src/a.rs", &only(&["*.rs", "!*.rs"]), rg), Some(false));
        assert_eq!(admitted("src/a.rs", &only(&["src/**"]), rg), None, "a folder in the filter is not read");
        let grep = |exclude: bool, glob: &str| NameFilter { exclude, glob: glob.to_string() };
        assert_eq!(admitted("src/a.rs", &[grep(false, "*.md")], Walk::Grep), Some(false));
        assert_eq!(admitted("docs/a.md", &[grep(false, "*.md")], Walk::Grep), Some(true));
        assert_eq!(admitted("docs/a.md", &[grep(true, "*.md")], Walk::Grep), Some(false));
        assert_eq!(admitted("src/a.rs", &[grep(true, "*.md")], Walk::Grep), Some(true));
    }

    #[test]
    fn a_file_type_is_a_set_of_name_filters_and_an_unknown_type_is_not_read() {
        let rust = type_filters("rust").expect("rust");
        assert_eq!(rust, [NameFilter { exclude: false, glob: "*.rs".into() }]);
        assert!(type_filters("ts").expect("ts").iter().any(|filter| filter.glob == "*.tsx"));
        assert_eq!(type_filters("fortran"), None);
    }

    #[test]
    fn the_session_state_belongs_to_the_session_and_to_each_agent_in_it() {
        let root = Path::new("/proj");
        let main = memory_path(root, Some("sessao-1"), None).expect("session");
        let agent = memory_path(root, Some("sessao-1"), Some("agente_2")).expect("agent");
        assert!(main.ends_with(".session/sessao-1/word-searches"), "{main:?}");
        assert!(agent.ends_with(".session/sessao-1/word-searches-agente_2"), "{agent:?}");
        assert_eq!(memory_path(root, None, None), None);
        assert_eq!(memory_path(root, Some("unknown"), None), None);
        assert_eq!(memory_path(root, Some("../fora"), None), None);
        assert_eq!(memory_path(root, Some("sessao-1"), Some("../fora")), None);
    }

    #[test]
    fn a_search_that_finds_every_word_in_the_strong_fields_answers_by_function() {
        let (_dir, root) = fixture::repo("{}");
        let text = answer(search_in(&root, &root, &["calcular_frete"], &["."]));
        assert!(text.starts_with("Cravado."), "{text}");
        assert!(text.contains(r#""calcular", "frete""#), "the words the map found: {text}");
        let frete = text.find("src/frete.rs\n  2-6 calcular_frete (2)").unwrap_or_else(|| panic!("{text}"));
        let pedido = text.find("src/pedido.rs\n  1-4 fechar_pedido (2)").unwrap_or_else(|| panic!("{text}"));
        assert!(frete < pedido, "the map's first file comes first: {text}");
        assert!(text.contains("docs/notas.md\n  1: O calcular_frete soma o imposto."), "a line outside any function comes loose: {text}");
        assert!(!text.contains("Fora do corte"), "nothing was cut: {text}");
    }

    /// A palavra que o mapa não traz não tira o cravado: o texto cita só as
    /// palavras que o primeiro achado tem em campo forte, sem pedir nova
    /// busca, e a linha com a palavra solta entra como achado do mesmo jeito.
    #[test]
    fn a_search_with_a_word_the_map_lacks_stays_pinned_and_names_only_the_found_words() {
        let (_dir, root) = fixture::repo("{}");
        let triaged =
            map_triage::triage_at(&store::model_path(&root), ("calcular_frete desconto_frete imposto", ""), &Languages::new(["pt-BR", "en-US"]), RANKED_FILES)
                .expect("triage");
        let chance = mustard_core::domain::triage::chance(&triaged.signals);
        assert!(chance >= mustard_core::domain::triage::PINNED_FROM + 0.01, "the fixture sits clear of the pinned cut, not on it: {chance}");
        let text = answer(search_in(&root, &root, &["calcular_frete|desconto_frete|imposto"], &["."]));
        assert!(text.starts_with("Cravado."), "{text}");
        assert!(text.contains(r#""calcular", "frete""#), "the words the map found: {text}");
        let first_line = text.lines().next().unwrap_or_default();
        assert!(!first_line.contains("imposto"), "the word the map lacks is not named: {first_line}");
        assert!(!text.contains("Falta"), "no search again is asked for: {text}");
        assert!(text.contains("src/frete.rs\n  2-6 calcular_frete (2, 3)"), "the comment with the word is a hit too: {text}");
    }

    /// A marca parcial com palavra fora dos campos fortes cita a que falta e
    /// pede nova busca; sem palavra faltando, diz só que não tem certeza.
    #[test]
    fn a_partial_mark_names_the_missing_words_and_without_them_says_it_is_unsure() {
        let answer = |missing: &[&str]| Triaged {
            grade: 4,
            signals: Signals { words: 2, strong: 1, first: Some(3.0), second: Some(2.5) },
            words: owned(&["calcular", "imposto"]),
            missing: owned(missing),
            files: Vec::new(),
            deeper: Vec::new(),
        };
        let with_missing = header(Mark::Partial, &answer(&["imposto"]), Locale::PtBr);
        assert!(with_missing.starts_with("Parcial.") && with_missing.contains(r#"Falta "imposto"."#), "{with_missing}");
        let without = header(Mark::Partial, &answer(&[]), Locale::PtBr);
        assert!(without.starts_with("Parcial.") && !without.contains("Falta"), "{without}");
    }

    #[test]
    fn a_pinned_search_that_only_lists_names_runs_plain_with_one_line_of_the_mark() {
        let (_dir, root) = fixture::repo("{}");
        let Reply::Note(line) = search_showing(&root, &root, &["calcular_frete"], &["."], false) else {
            panic!("a note was expected");
        };
        assert_eq!(line.lines().count(), 1, "{line}");
        assert!(line.starts_with("Cravado."), "{line}");
        assert!(line.contains("só lista nomes de arquivo ou conta"), "{line}");
        assert!(!line.contains("src/frete.rs"), "no answer by function goes with it: {line}");
        assert!(matches!(search_showing(&root, &root, &["calcular_frete"], &["."], false), Reply::Note(_)), "the repeat of a names search still gets the line");
        assert!(
            matches!(search_in(&root, &root, &["calcular_frete"], &["."]), Reply::Answer(_)),
            "the search that shows lines still gets the answer after a names search"
        );
    }

    #[test]
    fn a_pinned_search_with_a_missing_word_that_only_counts_answers_in_one_line_without_it() {
        let (_dir, root) = fixture::repo("{}");
        let Reply::Note(line) = search_showing(&root, &root, &["calcular_frete|desconto_frete|imposto"], &["."], false) else {
            panic!("a note was expected");
        };
        assert_eq!(line.lines().count(), 1, "{line}");
        assert!(line.starts_with("Cravado."), "{line}");
        assert!(!line.contains("imposto"), "{line}");
    }

    #[test]
    fn a_search_that_only_lists_names_and_finds_nothing_in_the_map_gets_the_not_found_line() {
        let (_dir, root) = fixture::repo("{}");
        let Reply::Note(line) = search_showing(&root, &root, &["zzznada"], &["."], false) else {
            panic!("a note was expected");
        };
        assert!(line.starts_with("Não achei"), "{line}");
    }

    #[test]
    fn a_search_the_map_finds_nothing_for_runs_plain_with_one_line() {
        let (_dir, root) = fixture::repo("{}");
        let first = search_in(&root, &root, &["zzznada"], &["."]);
        match &first {
            Reply::Note(line) => {
                assert_eq!(line.lines().count(), 1, "{line}");
                assert!(line.starts_with("Não achei"), "{line}");
                assert!(line.contains(r#"grep -rniE "zzznada" ."#), "{line}");
            }
            other => panic!("a note was expected, got {other:?}"),
        }
        assert_eq!(search_in(&root, &root, &["zzznada"], &["."]), Reply::Pass, "the repeat runs plain, with no line");
    }

    #[test]
    fn the_same_search_repeated_in_the_session_passes_and_another_folder_does_not() {
        let (_dir, root) = fixture::repo("{}");
        assert!(matches!(search_in(&root, &root, &["calcular_frete"], &["src"]), Reply::Answer(_)));
        assert_eq!(search_in(&root, &root, &["calcular_frete"], &["src"]), Reply::Pass, "the whole list comes from the plain search");
        assert!(matches!(search_in(&root, &root, &["calcular_frete"], &["."]), Reply::Answer(_)), "another folder is another search");
        assert!(matches!(search_in(&root, &root, &["fechar_pedido"], &["src"]), Reply::Answer(_)), "another pattern is another search");
    }

    #[test]
    fn without_a_session_state_the_search_is_not_remembered() {
        let (_dir, root) = fixture::repo("{}");
        let patterns = owned(&["calcular_frete"]);
        let folders = [project_path(&root.to_string_lossy(), &root.to_string_lossy(), "src").expect("src")];
        let search = Search {
            patterns: &patterns,
            dialect: Dialect::Rust,
            ignore_case: false,
            whole_word: false,
            folders: &folders,
            filters: &[],
            walk: Walk::Rg { unignored: false },
            shows_lines: true,
        };
        let config = ProjectConfig::load(&root);
        let scene = Scene {
            root: &root,
            model: &store::model_path(&root),
            memory: None,
            session: None,
            lang: Locale::PtBr,
            languages: &Languages::new(["pt-BR"]),
            config: &config,
            assemble: &without_key,
        };
        assert!(matches!(reply(&scene, &search), Reply::Answer(_)));
        assert!(matches!(reply(&scene, &search), Reply::Answer(_)));
    }

    #[test]
    fn a_file_changed_after_the_map_is_reread_and_flagged_with_its_new_lines() {
        let (_dir, root) = fixture::repo("{}");
        std::fs::write(root.join("src/frete.rs"), format!("// a\n// b\n// c\n{}", fixture::FRETE)).expect("edit");
        let text = answer(search_in(&root, &root, &["calcular_frete"], &["src"]));
        assert!(text.contains("src/frete.rs (mudado nesta onda)\n  5-9 calcular_frete (5)"), "the lines moved by three: {text}");
        assert!(text.contains("src/pedido.rs\n  1-4 fechar_pedido (2)"), "the file the wave did not touch is not flagged: {text}");
    }

    #[test]
    fn a_file_changed_and_not_known_to_the_map_before_has_no_function_lines() {
        let (_dir, root) = fixture::repo("{}");
        std::fs::write(root.join("src/novo.rs"), "fn usa() {\n    calcular_frete(1);\n}\n").expect("new file");
        let text = answer(search_in(&root, &root, &["calcular_frete"], &["src"]));
        assert!(text.contains("src/novo.rs\n  2:     calcular_frete(1);") || text.contains("src/novo.rs\n  2: calcular_frete(1);"), "{text}");
        assert!(!text.contains("novo.rs (mudado"), "a file the map never read is not a changed one: {text}");
    }

    #[test]
    fn a_working_copy_is_read_from_its_own_tree_with_the_lines_of_the_copy() {
        let (dir, root) = fixture::repo("{}");
        let copy = dir.path().parent().expect("parent").join(format!("copia-{}", std::process::id()));
        git(&root, &["worktree", "add", "-q", &copy.to_string_lossy(), "-b", "onda"]);
        let copy = std::fs::canonicalize(&copy).expect("copy");
        std::fs::write(copy.join("src/frete.rs"), format!("// a\n// b\n{}", fixture::FRETE)).expect("edit");
        let text = answer(search_in(&root, &copy, &["calcular_frete"], &["src"]));
        assert!(text.contains("src/frete.rs (mudado nesta onda)\n  4-8 calcular_frete (4)"), "{text}");
        assert!(text.contains("src/pedido.rs\n  1-4 fechar_pedido (2)"), "{text}");
        let main = answer(search_in(&root, &root, &["fechar_pedido|calcular_frete"], &["src"]));
        assert!(!main.contains("mudado"), "the main tree was not touched: {main}");
        git(&root, &["worktree", "remove", "--force", &copy.to_string_lossy()]);
    }

    #[test]
    fn the_cut_keeps_the_first_files_and_counts_what_it_removes() {
        let (_dir, root) = fixture::repo("{}");
        for n in 0..4 {
            std::fs::write(root.join(format!("src/extra_{n}.rs")), "fn usa() {\n    calcular_frete(1);\n}\n").expect("extra");
        }
        std::fs::write(root.join("src/many.rs"), "calcular_frete();\n".repeat(10)).expect("many");
        let text = answer(search_in(&root, &root, &["calcular_frete"], &["."]));
        let files = text.lines().filter(|line| !line.starts_with(' ') && line.contains(".rs") || line.contains(".md")).count();
        assert!(files >= 5, "{text}");
        let many = text.split("src/many.rs\n").nth(1).expect("many.rs is shown");
        assert_eq!(many.lines().take_while(|line| line.starts_with("  ")).count(), ENTRIES_PER_FILE, "{text}");
        assert!(text.trim_end().ends_with("Fora do corte, lugares: 5, arquivos: 4. Repita a busca para ver a lista inteira."), "{text}");
    }

    #[test]
    fn a_search_that_finds_no_line_still_points_to_the_files_of_the_map() {
        let (_dir, root) = fixture::repo("{}");
        let text = answer(search_in(&root, &root, &[r"calcular_frete\d+"], &["src"]));
        assert!(text.starts_with("Cravado."), "{text}");
        assert!(text.contains("A busca comum não acharia nenhuma linha com esse texto."), "{text}");
        assert!(text.contains("`src/frete.rs`"), "the file of the map comes named: {text}");
    }

    #[test]
    fn a_search_the_reading_does_not_understand_passes() {
        let (_dir, root) = fixture::repo("{}");
        assert_eq!(search_in(&root, &root, &["(calcular)\\1"], &["src"]), Reply::Pass, "a back reference is not read");
        assert_eq!(search_in(&root, &root, &["a", "b"], &["src"]), Reply::Pass, "one letter is no word");
        assert_eq!(search_in(&root, &root, &["calcular_frete"], &["docs"]), Reply::Pass, "a folder with no mapped code");
        let (_bare, bare) = fixture::repo("{}");
        std::fs::remove_file(store::model_path(&bare)).expect("no map");
        assert_eq!(search_in(&bare, &bare, &["calcular_frete"], &["src"]), Reply::Pass, "no map, no answer, no error");
    }

    /// Um filtro de mentira: guarda cada pedido e dá a chance de cada
    /// candidato pelo nome (`chances`, e 0,01 para os outros), com a chance
    /// de "nenhum destes" e a confiança dadas; ou falha com `error`.
    #[derive(Clone)]
    struct Judge {
        asked: std::rc::Rc<std::cell::RefCell<Vec<FilterRequest>>>,
        chances: Vec<(&'static str, f64)>,
        none: f64,
        confidence: f64,
        error: Option<FilterError>,
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
            let (verdict, kept) = judged(&scores, self.none, self.confidence, request.share);
            Ok(Filtered { verdict, kept, usage: FilterUsage::default() })
        }
    }

    impl Judge {
        /// O filtro seguro: dá `chances` e quase nenhuma a "nenhum destes".
        fn sure_of(chances: &[(&'static str, f64)]) -> Self {
            Self { asked: std::rc::Rc::default(), chances: chances.to_vec(), none: 0.01, confidence: 0.9, error: None }
        }

        /// O filtro que acha que nenhum candidato serve.
        fn finding_none() -> Self {
            Self { none: 0.9, ..Self::sure_of(&[]) }
        }

        fn failing(error: FilterError) -> Self {
            Self { error: Some(error), ..Self::sure_of(&[]) }
        }

        /// A montagem que entrega este filtro, como a chave no projeto.
        fn assemble(&self) -> impl Fn(&Path, &ProjectConfig) -> Result<Assembled, FilterError> + '_ {
            move |_, _| Ok(Assembled { name: "jev", filter: Box::new(self.clone()), warning: None })
        }

        fn calls(&self) -> usize {
            self.asked.borrow().len()
        }

        fn last(&self) -> FilterRequest {
            self.asked.borrow().last().cloned().expect("the filter was called")
        }
    }

    /// A palavra cravada responde da triagem: o filtro não é chamado, nem a
    /// montagem dele, e nenhum aviso de chave sai.
    #[test]
    fn a_pinned_word_never_reaches_the_filter() {
        let (_dir, root) = fixture::repo("{}");
        let judge = Judge::sure_of(&[]);
        let text = answer(search_through(&root, &root, &["calcular_frete"], &["."], true, &judge.assemble()));
        assert!(text.starts_with("Cravado."), "{text}");
        assert_eq!(judge.calls(), 0);
        let no_key = answer(search_through(&root, &root, &["fechar_pedido"], &["."], true, &without_key));
        assert!(!no_key.contains("chave"), "a pinned answer warns of no key: {no_key}");
    }

    /// A palavra parcial vai ao filtro num pedido só, com todos os candidatos
    /// do banco, e a resposta traz só a peça entregue, com as linhas achadas
    /// dentro dela; o que a busca achou fora dela vira a contagem do que
    /// ficou de fora, e a linha de usar as ferramentas fecha a resposta.
    #[test]
    fn a_partial_word_goes_to_the_filter_once_and_the_answer_shows_only_the_delivered_pieces() {
        let (_dir, root) = fixture::repo("{}");
        let judge = Judge::sure_of(&[("calcular_frete", 0.9)]);
        let text = answer(search_through(&root, &root, &["imposto"], &["."], true, &judge.assemble()));
        assert_eq!(judge.calls(), 1);
        let asked = judge.last();
        let bank = mustard_core::io::map_search::candidates(&root, "imposto", "imposto", &Languages::new(["pt-BR", "en-US"]), 100)
            .expect("the bank candidates");
        assert_eq!(bank.candidates.len(), 2, "the bank lists the two functions of the file");
        assert_eq!(asked.candidates, bank.candidates, "every candidate of the bank goes in one request");
        assert_eq!(asked.words, ["imposto"]);
        assert!(text.contains("src/frete.rs\n  2-6 calcular_frete (3)"), "the delivered piece with the line found inside it: {text}");
        assert!(!text.contains("desconto_frete") && !text.contains("fechar_pedido"), "only what the filter delivered: {text}");
        assert!(text.contains("Fora do corte, lugares: 1, arquivos: 1."), "the line in the note is left out: {text}");
        assert!(!text.contains("docs/notas.md"), "{text}");
        assert!(text.trim_end().ends_with(&mustard_core::translate("map.search.use_tools", Locale::PtBr)), "{text}");
    }

    /// A peça entregue vem na ordem da chance, mesmo sem linha achada nela.
    #[test]
    fn the_delivered_pieces_come_in_the_order_of_the_chance_even_without_a_line_found() {
        let (_dir, root) = fixture::repo("{}");
        let judge = Judge::sure_of(&[("desconto_frete", 0.6), ("calcular_frete", 0.35)]);
        let text = answer(search_through(&root, &root, &["imposto"], &["."], true, &judge.assemble()));
        let discount = text.find("\n  8-10 desconto_frete\n").unwrap_or_else(|| panic!("{text}"));
        let freight = text.find("\n  2-6 calcular_frete (3)\n").unwrap_or_else(|| panic!("{text}"));
        assert!(discount < freight, "the better piece comes first: {text}");
    }

    /// O filtro que acha que nada serve deixa a busca comum rodar, com a
    /// linha de não achei; a mesma busca repetida passa sem chamar o filtro
    /// de novo.
    #[test]
    fn a_word_the_filter_finds_nothing_for_lets_the_plain_search_run() {
        let (_dir, root) = fixture::repo("{}");
        let judge = Judge::finding_none();
        let Reply::Note(line) = search_through(&root, &root, &["imposto"], &["."], true, &judge.assemble()) else {
            panic!("a note was expected");
        };
        assert!(line.starts_with("Não achei") && line.contains(r#"grep -rniE "imposto" ."#), "{line}");
        assert_eq!(search_through(&root, &root, &["imposto"], &["."], true, &judge.assemble()), Reply::Pass);
        assert_eq!(judge.calls(), 1, "the repeat does not ask again");
    }

    /// A busca que só lista nomes ou conta segue sem o filtro.
    #[test]
    fn a_names_only_search_never_reaches_the_filter() {
        let (_dir, root) = fixture::repo("{}");
        let judge = Judge::sure_of(&[("calcular_frete", 0.9)]);
        let reply = search_through(&root, &root, &["imposto"], &["."], false, &judge.assemble());
        assert!(matches!(reply, Reply::Note(_)), "{reply:?}");
        assert_eq!(judge.calls(), 0);
    }

    /// Sem chave, a busca parcial responde com a triagem e o aviso do motivo,
    /// e a mesma sessão não o recebe de novo.
    #[test]
    fn without_a_key_a_partial_word_gets_the_triage_answer_and_one_warning_per_session() {
        let (_dir, root) = fixture::repo("{}");
        let warned = mustard_core::translate("map.search.missing_key", Locale::PtBr);
        let first = answer(search_in(&root, &root, &["imposto"], &["."]));
        assert!(first.starts_with("Parcial."), "the triage answer: {first}");
        assert!(first.trim_end().ends_with(&*warned), "{first}");
        let second = answer(search_in(&root, &root, &["frete pedido"], &["."]));
        assert!(second.starts_with("Parcial.") && !second.contains(&*warned), "the same session is not warned again: {second}");
    }

    /// O filtro que falha deixa a resposta da triagem, com o motivo dito uma
    /// vez por sessão.
    #[test]
    fn a_failing_filter_leaves_the_triage_answer_with_the_reason_once() {
        let (_dir, root) = fixture::repo("{}");
        let judge = Judge::failing(FilterError::Timeout);
        let first = answer(search_through(&root, &root, &["imposto"], &["."], true, &judge.assemble()));
        assert!(first.starts_with("Parcial."), "{first}");
        assert!(first.contains("tempo esgotado"), "{first}");
        let second = answer(search_through(&root, &root, &["frete pedido"], &["."], true, &judge.assemble()));
        assert!(second.starts_with("Parcial.") && !second.contains("tempo esgotado"), "{second}");
        assert_eq!(judge.calls(), 2);
    }

    /// A ferramenta `Grep` e o `grep` do terminal chegam à mesma porta: a
    /// mesma palavra dá a mesma resposta.
    #[test]
    fn the_grep_tool_and_the_terminal_grep_get_the_same_answer() {
        let judge = Judge::sure_of(&[("calcular_frete", 0.9)]);
        let both = Languages::new(["pt-BR", "en-US"]);
        let (_tool_dir, tool) = fixture::repo("{}");
        let (_shell_dir, shell) = fixture::repo("{}");
        let by_tool = search_as(&tool, &tool, (&["imposto"], &["."], true), (&both, (Dialect::Rust, Walk::Rg { unignored: false })), &judge.assemble());
        let by_shell = search_as(&shell, &shell, (&["imposto"], &["."], true), (&both, (Dialect::Basic, Walk::Grep)), &judge.assemble());
        assert!(matches!(by_tool, Reply::Answer(_)), "{by_tool:?}");
        assert_eq!(by_tool, by_shell);
        assert_eq!(judge.calls(), 2);
    }

    /// Num projeto com o texto em português e o código em inglês, a palavra
    /// em português chega ao nome em inglês pela documentação: o filtro lê a
    /// palavra como ela é, e não como pedaço de nome, e a peça certa volta.
    /// Num projeto de uma língua só, a palavra única segue como pedaço de nome.
    #[test]
    fn a_word_in_the_text_language_reaches_the_english_name_through_the_filter() {
        let map = serde_json::json!({ "modules": [
            { "path": "src/users.rs", "language": "rust", "loc": 4, "declarations": [
                { "kind": "struct", "name": "UserRepository", "line": 2, "end_line": 4,
                  "doc": "Repositório dos usuários do sistema." }
            ] },
            { "path": "src/orders.rs", "language": "rust", "loc": 3, "declarations": [
                { "kind": "struct", "name": "OrderService", "line": 1, "end_line": 3 }
            ] }
        ] });
        let files = [
            ("src/users.rs", "// usuários\npub struct UserRepository {\n    id: u32,\n}\n"),
            ("src/orders.rs", "pub struct OrderService {\n    id: u32,\n}\n"),
        ];
        let (_dir, root) = fixture::repo_with("{}", &files, map);
        let judge = Judge::sure_of(&[("UserRepository", 0.9)]);
        let mixed = Languages::new(["pt-BR", "en-US"]);
        let rg = (Dialect::Rust, Walk::Rg { unignored: false });
        let text = answer(search_as(&root, &root, (&["usuários"], &["."], true), (&mixed, rg), &judge.assemble()));
        assert_eq!(judge.last().phrase, "usuários", "the word is the request, in the text language");
        assert!(judge.last().candidates.iter().any(|candidate| candidate.name == "UserRepository"));
        assert!(text.contains("src/users.rs\n  2-4 UserRepository"), "the English name comes back: {text}");
        let (_same_dir, same) = fixture::repo("{}");
        let alone = Judge::sure_of(&[("calcular_frete", 0.9)]);
        let one = Languages::new(["pt-BR"]);
        let _ = search_as(&same, &same, (&["imposto"], &["."], true), (&one, rg), &alone.assemble());
        assert_eq!(alone.last().phrase, "pedaço de nome: imposto", "one language: the single word is a piece of a name");
    }

    /// A medida do tamanho da resposta contra o da busca comum, sobre buscas
    /// de verdade (`WORD_SEARCH_SEARCHES`: uma por linha, com o padrão, as
    /// pastas e o programa), numa árvore com mapa (`WORD_SEARCH_TREE`, um
    /// repositório git com `.claude/grain.db`). Grava em `WORD_SEARCH_OUT`
    /// uma linha por busca. Só roda por pedido.
    #[test]
    #[ignore = "measurement: reads WORD_SEARCH_SEARCHES, WORD_SEARCH_TREE and WORD_SEARCH_OUT"]
    fn measure_the_answer_size() {
        use std::io::Write;
        let read = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is set"));
        let tree = PathBuf::from(read("WORD_SEARCH_TREE"));
        let searches = std::fs::read_to_string(read("WORD_SEARCH_SEARCHES")).expect("searches");
        let mut out = std::fs::File::create(read("WORD_SEARCH_OUT")).expect("out");
        let root = tree.to_string_lossy().into_owned();
        let languages = Languages::new(["pt-BR", "en-US"]);
        let model = store::model_path(&tree);
        for (at, line) in searches.lines().enumerate() {
            let entry: serde_json::Value = serde_json::from_str(line).expect("a search line");
            let patterns: Vec<String> = entry["patterns"].as_array().expect("patterns").iter().filter_map(|p| p.as_str().map(str::to_string)).collect();
            let folders: Vec<ProjectPath> = entry["folders"]
                .as_array()
                .expect("folders")
                .iter()
                .filter_map(|folder| folder.as_str())
                .filter(|folder| !folder.contains('>') && !folder.starts_with('&'))
                .filter_map(|folder| project_path(&root, &root, folder))
                .filter(|folder| folder.abs.is_dir())
                .collect();
            let wanted = entry["folders"].as_array().map_or(0, |all| all.iter().filter_map(|f| f.as_str()).filter(|f| !f.contains('>') && !f.starts_with('&')).count());
            let program = entry["program"].as_str().unwrap_or("grep");
            let dialect = match entry["dialect"].as_str() {
                Some("basic") => Dialect::Basic,
                Some("extended") => Dialect::Extended,
                Some("fixed") => Dialect::Fixed,
                _ => Dialect::Rust,
            };
            let walk = if program == "grep" || program == "egrep" || program == "fgrep" { Walk::Grep } else { Walk::Rg { unignored: false } };
            let search = Search {
                patterns: &patterns,
                dialect,
                ignore_case: entry["ci"].as_bool().unwrap_or(false),
                whole_word: entry["word"].as_bool().unwrap_or(false),
                folders: &folders,
                filters: &[],
                walk,
                shows_lines: entry["mode"].as_str() == Some("content"),
            };
            let config = ProjectConfig::load(&tree);
            let scene = Scene {
                root: &tree,
                model: &model,
                memory: None,
                session: None,
                lang: Locale::PtBr,
                languages: &languages,
                config: &config,
                assemble: &without_key,
            };
            let mut row = serde_json::json!({ "at": at, "program": program, "outcome": "pass", "folders_ok": !folders.is_empty() && folders.len() == wanted });
            if folders.is_empty() || folders.len() != wanted {
                writeln!(out, "{row}").expect("write");
                continue;
            }
            let started = Instant::now();
            let got = reply(&scene, &search);
            row["ms"] = serde_json::json!(started.elapsed().as_millis());
            let rels: Vec<String> = folders.iter().map(|folder| folder.rel.clone()).collect();
            if let Some(regex) = pattern_of(&search)
                && let Some(hits) = scan(&tree, &rels, &search, &regex)
            {
                let (mut content, mut names, mut counts, mut places) = (0usize, 0usize, 0usize, 0usize);
                for file in &hits {
                    let text = std::fs::read_to_string(tree.join(&file.path)).unwrap_or_default();
                    let rows: Vec<&str> = text.lines().collect();
                    names += file.path.len() + 1;
                    counts += file.path.len() + 1 + file.lines.len().to_string().len() + 1;
                    places += file.lines.len();
                    for &n in &file.lines {
                        let row = rows.get(n as usize - 1).copied().unwrap_or_default();
                        content += file.path.len() + 1 + n.to_string().len() + 1 + row.len() + 1;
                    }
                }
                row["plain"] = serde_json::json!({ "content": content, "files": names, "count": counts, "files_hit": hits.len(), "places": places });
            }
            match got {
                Reply::Answer(text) => {
                    row["outcome"] = serde_json::json!("answer");
                    row["mark"] = serde_json::json!(text.split('.').next().unwrap_or_default());
                    row["answer_bytes"] = serde_json::json!(text.len());
                    row["cut"] = serde_json::json!(text.contains("Fora do corte"));
                    row["map_only"] = serde_json::json!(text.contains("nenhuma linha"));
                }
                Reply::Note(text) => {
                    row["outcome"] = serde_json::json!("note");
                    row["note_bytes"] = serde_json::json!(text.len());
                }
                Reply::Pass => {}
            }
            writeln!(out, "{row}").expect("write");
        }
    }
}
