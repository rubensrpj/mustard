//! `map` — perguntas curtas ao mapa do projeto que o scan grava.
//!
//! `mustard-rt run map <pergunta>`:
//! - `examples --file <alvo>` (ou `--task "<tarefa>"`): 2 ou 3 arquivos que
//!   servem de exemplo, com o motivo de cada um e as receitas do git; a do
//!   arquivo cujo último commit ficou fora da janela do mapa sai da história
//!   dele lida do git na hora, e fica gravada no mapa;
//! - `importers --file <arquivo>`: quem importa o arquivo;
//! - `tests --file <arquivo>`: que testes o cobrem;
//! - `slice --file <arquivo> --name <declaração>`: o trecho da declaração, do
//!   começo ao fim, com o caminho e as linhas de onde ele saiu; perguntado de
//!   dentro de uma cópia de trabalho do projeto, o do arquivo da cópia;
//! - `users --name <declaração>` (com `--file`, só a desse arquivo): quem usa
//!   a declaração, como `arquivo:linha:quem chama` — as ligações provadas
//!   primeiro, e as suspeitas agrupadas pelas declarações que a chamada pode
//!   alcançar; na função que atende uma rota do servidor, também a rota e as
//!   chamadas da tela que a alcançam;
//! - `history --name <declaração>` (com `--file`, só a desse arquivo): os
//!   commits da branch de partida que mudaram a declaração, do mais novo ao
//!   mais velho, com o título e o número do pull request; a lista de cada
//!   arquivo se monta na primeira pergunta sobre ele e fica gravada no mapa;
//! - `search --query "<palavras>" --intent "<frase>"`: a busca por
//!   conceito, nos arquivos e nos itens combinados das specs, cada um com a
//!   função ou o arquivo ligado a ele. Com o filtro, os candidatos do banco
//!   ganham a nota dele contra a frase, e a resposta traz as peças que
//!   passaram, com o que cada uma puxou pelas ligações do mapa;
//! - `summary`: o resumo do início da sessão, até 3 kB; com `--file`, as
//!   partes do arquivo — cada declaração fora dos testes, com o tipo, o nome
//!   e as linhas, e a linha em que os testes começam;
//! - `skill --path <SKILL.md>`: confere os caminhos que a skill cita e o
//!   tamanho dela;
//! - `dump`: o banco do mapa tabela por tabela, em ordem fixa, para depurar.
//!
//! A regra mora em `mustard_core::domain::project_map`, e a leitura do banco
//! na porta `mustard_core::io::project_map`; a busca lê o índice de palavras
//! do mapa, por `mustard_core::io::map_search`, sem o mapa inteiro. Aqui só
//! se leem o mapa, a skill e o arquivo de onde sai o trecho, e se imprime o
//! JSON.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::ValueEnum;
use mustard_core::domain::map_filter::{FilterCandidate, FilterError, FilterRequest, Filtered, MapFilter, CUT_MINIMUM};
use mustard_core::domain::map_select::{capped, select, Source, MAX_RETURNED};
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::project_map::{self as project_map, DeclAt, MapRefusal, ProjectMap, UseSite};
use mustard_core::domain::scan::{HistoryReport, ScanReport};
use mustard_core::domain::search::{CANDIDATES, TOP};
use mustard_core::io::{map_search, map_specs};
use mustard_core::io::project_map::{self as store, Need};
use mustard_core::io::workspace::{is_git_repo_root, linked_worktree_main};
use mustard_core::io::wave_prompt::{history_beyond_window, recipe_on_disk};
use mustard_core::platform::i18n::Locale;
use mustard_core::{FilterSetting, Setting};
use serde_json::{json, Map, Value};

/// A pergunta feita ao mapa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Question {
    Examples,
    Importers,
    Tests,
    Search,
    Summary,
    Skill,
    Slice,
    Users,
    History,
    Dump,
}

impl Question {
    fn name(self) -> &'static str {
        match self {
            Self::Examples => "examples",
            Self::Importers => "importers",
            Self::Tests => "tests",
            Self::Search => "search",
            Self::Summary => "summary",
            Self::Skill => "skill",
            Self::Slice => "slice",
            Self::Users => "users",
            Self::History => "history",
            Self::Dump => "dump",
        }
    }
}

/// As opções de `mustard-rt run map`.
pub struct MapOpts {
    pub root: PathBuf,
    pub question: Question,
    pub file: Option<String>,
    pub task: Option<String>,
    pub query: Option<String>,
    /// A frase do que se procura e para quê, que só o filtro lê.
    pub intent: Option<String>,
    pub path: Option<PathBuf>,
    pub name: Option<String>,
    /// O pull request cuja descrição a história mostra.
    pub pr: Option<u32>,
    /// A sessão de quem pergunta, que guarda os avisos já dados.
    pub session: Option<String>,
}

fn refused(refusal: &MapRefusal, lang: Locale) -> Value {
    json!({ "ok": false, "reason": refusal.reason(), "hint": refusal.message(lang) })
}

/// O valor de uma opção que a pergunta exige, ou a recusa que diz qual falta.
fn required(value: Option<&str>, question: Question, flag: &str) -> Result<String, MapRefusal> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| MapRefusal::MissingArgument { question: question.name().to_string(), flag: flag.to_string() })
}

/// A releitura do mapa: roda o scan sobre a raiz, gravando no caminho dado.
pub(crate) type Mine<'m> = dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport> + 'm;

/// A passada da história por arquivo: lê do git a história das declarações
/// do arquivo (o terceiro argumento) na raiz, seguindo cada uma até o número
/// de mudanças de arquivo do quarto, e a grava no mapa do caminho dado.
pub(crate) type Trace<'t> =
    dyn Fn(&Path, &Path, &str, usize) -> mustard_core::platform::error::Result<HistoryReport> + 't;

/// O filtro da busca montado com a chave do projeto: o nome dele, que a
/// resposta e o registro da chamada dizem, e o aviso da chave, quando a do
/// ambiente vale mas o git guarda o `mustard.json` que também traz uma.
pub(crate) struct Assembled {
    pub(crate) name: &'static str,
    pub(crate) filter: Box<dyn MapFilter>,
    pub(crate) warning: Option<FilterError>,
}

/// A montagem do filtro para o projeto na raiz, com o `mustard.json` que a
/// busca leu: com a chave, o filtro; sem ela, o motivo, que vira aviso. Só a
/// busca por assunto a chama, e só quando a configuração não desliga o
/// filtro.
pub(crate) type Assemble<'a> = dyn Fn(&Path, &mustard_core::ProjectConfig) -> Result<Assembled, FilterError> + 'a;

/// A montagem de verdade: o Jev, com a chave do ambiente ou do
/// `mustard.json` do projeto.
fn jev(root: &Path, config: &mustard_core::ProjectConfig) -> Result<Assembled, FilterError> {
    let loaded = crate::shared::jev::load_key(root, config)?;
    Ok(Assembled {
        name: "jev",
        filter: Box::new(crate::shared::jev::JevFilter::new(loaded.key)),
        warning: loaded.warning,
    })
}

/// Responde a pergunta e devolve o JSON; nunca entra em pânico. Antes, a
/// conferência do mapa com o conteúdo de agora relê por `mine` o que mudou
/// desde a passada que o gravou, e o bloco das specs recebe o que as specs
/// gravaram desde a última resposta; a falha dele deixa o bloco como
/// estava, e a resposta sai. A pergunta da história monta por `trace` a
/// lista do arquivo que o mapa ainda não tem, ou que venceu, e a dos
/// exemplos, a do alvo além da janela do mapa; a busca monta por `trace` a
/// dos arquivos ligados aos itens achados, e por `assemble` o filtro, quando
/// a configuração não o desliga.
pub(crate) fn map_at(opts: &MapOpts, mine: &Mine<'_>, trace: &Trace<'_>, assemble: &Assemble<'_>) -> Value {
    let project = crate::commands::spec_events::project(&opts.root);
    crate::commands::flow::round::refresh_map_if_stale(&project.root, mine);
    let _ = map_specs::sync(&project.root, &project.languages);
    let lang = project.lang;
    match answer(opts, &project.root, lang, &project.languages, trace, assemble) {
        Ok(report) => report,
        Err(refusal) => refused(&refusal, lang),
    }
}

/// Antes da busca, a história por função dos arquivos que ligam os itens
/// achados das specs: o arquivo que um commit de onda do item mudou e que
/// ainda não tem essa história, ou cuja história venceu pela regra da
/// pergunta da história, entraria inteiro na resposta, no lugar da função.
/// Cada um se lê por `trace`, do git local, um arquivo por vez, e fica
/// gravado no mapa; a busca seguinte já o acha, sem passada. A falha de um
/// deixa o arquivo inteiro, e a busca responde assim mesmo. A história segue
/// até `moves` mudanças de arquivo seguidas.
fn trace_search_links(root: &Path, query: &str, languages: &Languages, moves: usize, trace: &Trace<'_>) {
    let query = query.trim();
    if query.is_empty() {
        return;
    }
    let Ok(found) = map_search::search_specs(root, query, languages, TOP) else { return };
    if found.is_empty() {
        return;
    }
    let items: Vec<(&str, &str)> = found.iter().map(|item| (item.spec.as_str(), item.code.as_str())).collect();
    let Ok(files) = map_specs::untraced(root, &items, TOP, moves) else { return };
    let model = store::model_path(root);
    for file in &files {
        let _ = trace(root, &model, file, moves);
    }
}

/// O banco do mapa tabela por tabela, como a porta o lê.
fn dump(root: &Path) -> Result<Value, MapRefusal> {
    Ok(json!({ "ok": true, "question": "dump", "tables": store::dump(root)? }))
}

fn answer(
    opts: &MapOpts,
    root: &Path,
    lang: Locale,
    languages: &Languages,
    trace: &Trace<'_>,
    assemble: &Assemble<'_>,
) -> Result<Value, MapRefusal> {
    answer_from(opts, root, lang, languages, &|need| store::read_for(root, need), trace, assemble)
}

/// Como o mapa se lê para uma pergunta: pela porta, só as tabelas de que ela
/// precisa.
type Reader<'r> = dyn Fn(Need<'_>) -> Result<ProjectMap, MapRefusal> + 'r;

/// O valor de uma opção que a pergunta exige; sem ele, primeiro as recusas
/// do mapa, como quando o mapa se lia antes de olhar as opções, e depois a
/// que diz qual opção falta.
fn after_the_map<T>(value: Result<T, MapRefusal>, read: &Reader<'_>) -> Result<T, MapRefusal> {
    value.or_else(|missing| {
        read(Need::Nothing)?;
        Err(missing)
    })
}

/// A resposta da pergunta com o mapa lido por `read`.
fn answer_from(
    opts: &MapOpts,
    root: &Path,
    lang: Locale,
    languages: &Languages,
    read: &Reader<'_>,
    trace: &Trace<'_>,
    assemble: &Assemble<'_>,
) -> Result<Value, MapRefusal> {
    let question = opts.question;
    match question {
        Question::Importers => {
            let file = after_the_map(required(opts.file.as_deref(), question, "--file"), read)?;
            let importers = project_map::importers(&read(Need::Importers(&file))?, &file)?;
            Ok(json!({ "ok": true, "question": "importers", "file": project_map::clean_path(&file), "importers": importers }))
        }
        Question::Tests => {
            let file = after_the_map(required(opts.file.as_deref(), question, "--file"), read)?;
            let tests = project_map::tests_for(&read(Need::Tests(&file))?, &file)?;
            Ok(json!({
                "ok": true,
                "question": "tests",
                "file": project_map::clean_path(&file),
                "inline": tests.inline,
                "tests": tests.files,
            }))
        }
        Question::Search => search(opts, root, lang, languages, read, (trace, assemble)),
        Question::Summary => match opts.file.as_deref().map(str::trim).filter(|file| !file.is_empty()) {
            Some(file) => parts(file, read),
            None => {
                let text = project_map::summary(&read(Need::Summary)?, lang);
                Ok(json!({ "ok": true, "question": "summary", "bytes": text.len(), "summary": text }))
            }
        },
        Question::Slice => slice(opts, root, read),
        Question::Users => users(opts, root, lang, read),
        Question::History => history(opts, root, lang, read, trace),
        Question::Examples => examples(opts, root, lang, languages, read, trace),
        Question::Skill => skill(opts, root, read),
        Question::Dump => dump(root),
    }
}

/// Os números da busca com filtro, com o padrão no lugar do ausente e do
/// inválido.
struct SearchNumbers {
    candidates: usize,
    cut_min: usize,
    max_returned: usize,
}

/// A busca por assunto. O filtro se escolhe aqui, num ponto só: desligado
/// (`none`) ou com nome desconhecido, nenhum; ausente ou `jev`, o que
/// `assemble` monta, se há chave para o projeto; sem ela, nenhum, com o
/// aviso do motivo. Com filtro, os candidatos do
/// banco passam pela nota dele, a resposta traz as peças, e a chamada fica
/// gravada na spec atual com o tempo, os tokens e o custo; sem filtro, ou na
/// falha dele, a resposta é a da busca do banco. Antes dela, `trace` monta a
/// história dos arquivos ligados aos itens achados, seguindo o número de
/// `map.historyMoves` lido como a pergunta da história o lê. Os avisos saem
/// uma vez por sessão.
fn search(
    opts: &MapOpts,
    root: &Path,
    lang: Locale,
    languages: &Languages,
    read: &Reader<'_>,
    (trace, assemble): (&Trace<'_>, &Assemble<'_>),
) -> Result<Value, MapRefusal> {
    let started = Instant::now();
    let query = after_the_map(required(opts.query.as_deref(), opts.question, "--query"), read)?;
    let intent = opts.intent.as_deref().map(str::trim).unwrap_or_default();
    let session = opts.session.as_deref();
    let config = mustard_core::ProjectConfig::load(root);
    let mut warnings: Vec<String> = Vec::new();
    let moves = history_moves(root, session, lang, &config, &mut warnings);
    trace_search_links(root, &query, languages, moves, trace);
    let mut number = |key: &str, setting: Setting, default: usize| {
        if setting == Setting::Invalid && first_warning(root, session, &format!("search.{key}")) {
            warnings.push(
                mustard_core::translate("map.search.bad_number", lang)
                    .replace("{key}", key)
                    .replace("{default}", &default.to_string()),
            );
        }
        setting.or(default)
    };
    let numbers = SearchNumbers {
        candidates: number("candidates", config.search_candidates(), CANDIDATES),
        cut_min: number("cut_min", config.search_cut_min(), CUT_MINIMUM),
        max_returned: number("max_returned", config.search_max_returned(), MAX_RETURNED),
    };
    let assembled = match config.search_filter() {
        FilterSetting::Off => None,
        FilterSetting::Invalid => {
            if first_warning(root, session, "search.filter") {
                warnings.push(mustard_core::translate("map.search.bad_filter", lang).to_string());
            }
            None
        }
        FilterSetting::Absent | FilterSetting::Jev => match assemble(root, &config) {
            Ok(assembled) => Some(assembled),
            Err(error) => {
                key_warning(root, session, &error, lang, &mut warnings);
                None
            }
        },
    };
    let (mut report, measured) = match assembled {
        None => (bank_search(root, &query, languages)?, None),
        Some(assembled) => {
            if let Some(error) = &assembled.warning {
                key_warning(root, session, error, lang, &mut warnings);
            }
            let asked = Asked { root, query: &query, intent, lang, languages, numbers: &numbers };
            let searched = filtered_search(&asked, &assembled)?;
            if let Some(error) = searched.failure
                && first_warning(root, session, "search.filter_failed")
            {
                let reason = mustard_core::translate(&format!("map.search.reason.{}", error.reason()), lang);
                warnings.push(mustard_core::translate("map.search.filter_failed", lang).replace("{reason}", reason));
            }
            (searched.report, Some(searched.measured))
        }
    };
    // Os itens das specs que casam, cada um com o código, o título, a linha
    // da parte do usuário que casou e os lugares ligados.
    let items: Vec<Value> = map_search::search_specs(root, &query, languages, TOP)?
        .into_iter()
        .map(|item| {
            let mut found = json!({ "spec": item.spec, "code": item.code, "title": item.title });
            if let Some(line) = item.line {
                found["line"] = json!(line);
            }
            if !item.links.is_empty() {
                found["links"] = json!(item.links);
            }
            found
        })
        .collect();
    if !items.is_empty() {
        report["specs"] = json!(items);
    }
    if !warnings.is_empty() {
        report["warnings"] = json!(warnings);
    }
    if let Some(measured) = measured {
        let _ = crate::commands::spec_events::conversation::record_measured_call(
            root,
            "map search",
            None,
            session,
            started,
            &report,
            measured,
        );
    }
    Ok(report)
}

/// O aviso da chave do filtro, uma vez por sessão para cada motivo: a chave
/// que falta, ou a do `mustard.json` que o git guarda. Nenhum dos dois leva
/// a chave.
fn key_warning(root: &Path, session: Option<&str>, error: &FilterError, lang: Locale, warnings: &mut Vec<String>) {
    let key = match error {
        FilterError::KeyInGit => "map.search.key_in_git",
        _ => "map.search.missing_key",
    };
    if first_warning(root, session, &format!("search.{}", error.reason())) {
        warnings.push(mustard_core::translate(key, lang).to_string());
    }
}

/// A busca do banco, sem filtro: os arquivos que mais casam com a pergunta.
fn bank_search(root: &Path, query: &str, languages: &Languages) -> Result<Value, MapRefusal> {
    let files: Vec<Value> = map_search::search(root, query, languages, TOP)?
        .into_iter()
        .map(|found| {
            let mut file = json!({ "path": found.path, "score": found.score });
            // O texto fixo que casou vem com a linha e a declaração onde
            // nasce.
            if let Some(text) = found.text {
                file["text"] = json!({
                    "line": text.line, "kind": text.kind, "value": text.value, "owner": text.owner
                });
            }
            file
        })
        .collect();
    Ok(json!({ "ok": true, "question": "search", "query": query, "files": files }))
}

/// O que a busca com filtro leva: o projeto, as palavras, a frase, o idioma
/// do texto, as línguas das palavras e os números.
struct Asked<'a> {
    root: &'a Path,
    query: &'a str,
    intent: &'a str,
    lang: Locale,
    languages: &'a Languages,
    numbers: &'a SearchNumbers,
}

/// A frase que o filtro lê: a de `--intent`; sem ela, as palavras da
/// `--query`, e a palavra só vira o pedido de um pedaço de nome, no idioma do
/// texto do projeto.
fn phrase_of(words: &[String], query: &str, intent: &str, lang: Locale) -> String {
    match (intent.is_empty(), words) {
        (false, _) => intent.to_string(),
        (true, [word]) => mustard_core::translate("map.search.name_piece", lang).replace("{word}", word),
        (true, _) => query.to_string(),
    }
}

/// O que a busca com o filtro devolve: a resposta, os campos da medida da
/// chamada e, na falha do filtro, o erro dele.
struct Searched {
    report: Value,
    measured: Map<String, Value>,
    failure: Option<FilterError>,
}

/// A busca com o filtro montado; na falha dele, a resposta é a da busca do
/// banco.
fn filtered_search(asked: &Asked<'_>, assembled: &Assembled) -> Result<Searched, MapRefusal> {
    let found = map_search::candidates(asked.root, asked.query, asked.intent, asked.languages, asked.numbers.candidates)?;
    let words: Vec<String> = asked.query.split_whitespace().map(str::to_string).collect();
    let phrase = phrase_of(&words, asked.query, asked.intent, asked.lang);
    let request = FilterRequest { words, phrase, minimum: asked.numbers.cut_min, candidates: found.candidates };
    let calling = Instant::now();
    let mut measured = Map::new();
    measured.insert("candidates".to_string(), json!(request.candidates.len()));
    match assembled.filter.filter(&request) {
        Ok(filtered) => {
            let pieces = pieces(asked.root, &request.candidates, &found.whole, &filtered, asked.numbers.max_returned)?;
            measured.insert("filter".to_string(), json!(assembled.name));
            measured.insert("filter_ms".to_string(), json!(filtered.usage.millis));
            measured.insert("tokens".to_string(), json!(filtered.usage.input_tokens));
            measured.insert("cost_micro_usd".to_string(), json!(filtered.usage.cost_micro_usd));
            measured.insert("returned".to_string(), json!(pieces.len()));
            if !filtered.usage.model.is_empty() {
                measured.insert("model".to_string(), json!(filtered.usage.model));
            }
            let report = json!({
                "ok": true, "question": "search", "query": asked.query, "filter": assembled.name, "pieces": pieces
            });
            Ok(Searched { report, measured, failure: None })
        }
        Err(error) => {
            let report = bank_search(asked.root, asked.query, asked.languages)?;
            measured.insert("filter".to_string(), json!(format!("{}:{}", assembled.name, error.reason())));
            measured.insert("filter_ms".to_string(), json!(u64::try_from(calling.elapsed().as_millis()).unwrap_or(u64::MAX)));
            measured.insert("returned".to_string(), json!(report["files"].as_array().map_or(0, Vec::len)));
            Ok(Searched { report, measured, failure: Some(error) })
        }
    }
}

/// As peças da resposta com filtro, na ordem da combinação e até o teto
/// `max`: o corte, o que cada item dele puxou e os primeiros do banco. Cada
/// peça traz o caminho, a linha, o fim, o tipo, o nome, a assinatura e a
/// primeira frase da documentação; só as do corte trazem a nota. Nunca o
/// corpo.
fn pieces(
    root: &Path,
    candidates: &[FilterCandidate],
    whole: &[i64],
    filtered: &Filtered,
    max: usize,
) -> Result<Vec<Value>, MapRefusal> {
    let cut: Vec<i64> = filtered.kept.iter().map(|scored| scored.id).collect();
    let scores: HashMap<i64, f64> = filtered.kept.iter().map(|scored| (scored.id, scored.score)).collect();
    let bank: Vec<i64> = candidates.iter().map(|candidate| candidate.id).collect();
    let picks = capped(&select(&cut, &bank, whole, &map_search::links(root, &cut)?), max);
    let outside: Vec<i64> = picks.iter().map(|pick| pick.id).filter(|id| !bank.contains(id)).collect();
    let pulled = map_search::declarations(root, &outside)?;
    let known: HashMap<i64, &FilterCandidate> =
        candidates.iter().chain(&pulled).map(|candidate| (candidate.id, candidate)).collect();
    Ok(picks
        .iter()
        .filter_map(|pick| {
            let decl = known.get(&pick.id)?;
            let mut piece = json!({
                "path": decl.path, "line": decl.line, "end_line": decl.end_line, "kind": decl.kind, "name": decl.name
            });
            if !decl.signature.is_empty() {
                piece["signature"] = json!(decl.signature);
            }
            // O começo da documentação: a primeira frase, como a de um item
            // de spec.
            let doc = project_map::spec_sentence(&decl.documentation);
            if !doc.is_empty() {
                piece["doc"] = json!(doc);
            }
            if pick.source == Source::Cut
                && let Some(score) = scores.get(&pick.id)
            {
                piece["score"] = json!((score * 100.0).round() / 100.0);
            }
            Some(piece)
        })
        .collect())
}

/// As partes do arquivo de `--file`, para quem vai ler só um trecho dele: cada
/// declaração fora dos testes, com o tipo, o nome e as linhas de começo e de
/// fim, e a linha em que os testes escritos dentro dele começam, quando há.
fn parts(file: &str, read: &Reader<'_>) -> Result<Value, MapRefusal> {
    let found = project_map::parts(&read(Need::Parts(file))?, file)?;
    Ok(json!({
        "ok": true,
        "question": "summary",
        "file": found.file,
        "parts": found.parts,
        "tests_line": found.tests_line,
    }))
}

/// O trecho da declaração de `--name` no arquivo de `--file`: as linhas dela,
/// do começo ao fim, mais o caminho e as linhas de onde saíram. O mapa diz
/// onde a declaração mora; o arquivo é lido aqui, uma vez, para que quem
/// pergunta não precise abri-lo. Numa cópia de trabalho do projeto, como a
/// de uma onda, o arquivo é o da cópia ([`slice_in_copy`]).
fn slice(opts: &MapOpts, root: &Path, read: &Reader<'_>) -> Result<Value, MapRefusal> {
    let question = opts.question;
    let file = after_the_map(required(opts.file.as_deref(), question, "--file"), read)?;
    let name = after_the_map(required(opts.name.as_deref(), question, "--name"), read)?;
    let map = read(Need::Declarations { file: Some(&file), name: &name })?;
    let place = project_map::declaration(&map, &file, &name)?;
    let (line, end_line) = (place.line, place.end_line);
    let unreadable = |e: std::io::Error| MapRefusal::FileUnreadable { file: place.file.clone(), detail: e.to_string() };
    let project_text = std::fs::read_to_string(root.join(&place.file));
    let (text, line, end_line) = match working_copy(&opts.root, root) {
        None => (project_text.map_err(unreadable)?, line, end_line),
        Some(copy) => {
            let text = std::fs::read_to_string(copy.join(&place.file)).map_err(unreadable)?;
            let (line, end_line) = slice_in_copy(&place, &text, project_text.ok().as_deref())?;
            (text, line, end_line)
        }
    };
    Ok(json!({
        "ok": true,
        "question": "slice",
        "file": place.file,
        "name": place.name,
        "kind": place.kind,
        "line": line,
        "end_line": end_line,
        "doc": place.doc,
        "signature": place.signature,
        "slice": project_map::lines_of(&text, line, end_line),
    }))
}

/// A raiz da cópia de trabalho de onde a pergunta vem (`start`), quando ela
/// é uma cópia ligada ao repositório do projeto `root`, como a de uma onda.
/// `None` no próprio projeto e em qualquer outra pasta. Lê só os arquivos
/// que o git deixa, sem rodar o git.
fn working_copy(start: &Path, root: &Path) -> Option<PathBuf> {
    let start = std::path::absolute(start).ok()?;
    let top = start.ancestors().find(|folder| is_git_repo_root(folder))?;
    let main = linked_worktree_main(top)?;
    let same = match (std::fs::canonicalize(&main), std::fs::canonicalize(root)) {
        (Ok(main), Ok(root)) => main == root,
        _ => false,
    };
    same.then(|| top.to_path_buf())
}

/// As linhas da declaração `place` no texto `copy` do arquivo numa cópia de
/// trabalho, dado o texto `project` do mesmo arquivo no projeto, de onde o
/// mapa tirou as linhas. Com os dois iguais, as linhas do mapa valem. Com o
/// arquivo mudado na cópia, a declaração do projeto, linha a linha, acha o
/// lugar dela na cópia: onde ela aparece inteira, a vez mais perto da linha
/// do mapa. Sem ela inteira na cópia, a declaração mudou ali, e a recusa
/// [`MapRefusal::ChangedInCopy`] manda ler o arquivo da cópia por faixa de
/// linhas.
fn slice_in_copy(place: &project_map::DeclPlace, copy: &str, project: Option<&str>) -> Result<(u64, u64), MapRefusal> {
    if project == Some(copy) {
        return Ok((place.line, place.end_line));
    }
    let (line, end_line) = (place.line.max(1), place.end_line.max(place.line.max(1)));
    let changed = || MapRefusal::ChangedInCopy { file: place.file.clone(), name: place.name.clone(), line };
    let block: Vec<&str> = project
        .unwrap_or_default()
        .lines()
        .skip(usize::try_from(line - 1).unwrap_or(usize::MAX))
        .take(usize::try_from(end_line - line + 1).unwrap_or(usize::MAX))
        .collect();
    if block.is_empty() {
        return Err(changed());
    }
    let lines: Vec<&str> = copy.lines().collect();
    let first = (1..)
        .zip(lines.windows(block.len()))
        .filter(|(_, window)| *window == block.as_slice())
        .map(|(first, _): (u64, _)| first)
        .min_by_key(|first| first.abs_diff(line))
        .ok_or_else(changed)?;
    Ok((first, first + (end_line - line)))
}

/// Quem usa a declaração de `--name`: cada declaração com esse nome no mapa
/// (só a do arquivo de `--file`, quando ele vem), com os usos que o scan
/// gravou, como `arquivo:linha:quem chama`. As ligações provadas vêm primeiro,
/// em `used_by`; as suspeitas, em `suspect`, agrupadas pelas declarações que a
/// chamada pode alcançar, e a resposta com alguma traz em `next` o próximo
/// passo: conferir cada uma pelo servidor de linguagem. As chamadas que o
/// nome comum demais deixou sem ligação vêm só contadas, com o jeito de
/// achá-las. A declaração que atende rotas do servidor traz, em `routes`,
/// cada uma com o arquivo e a linha dela e as chamadas da tela que a
/// alcançam, como `arquivo:linha:quem chama`: as provadas em `screens`, e as
/// suspeitas em `suspect`, agrupadas pelas funções que a chamada pode
/// alcançar. A que ninguém usa leva a nota que diz isso, para que a lista
/// vazia não pareça um mapa sem a informação. O teto do nome comum escrito
/// errado no `mustard.json` sai em `warning`, uma vez por sessão.
fn users(opts: &MapOpts, root: &Path, lang: Locale, read: &Reader<'_>) -> Result<Value, MapRefusal> {
    let name = after_the_map(required(opts.name.as_deref(), opts.question, "--name"), read)?;
    let file = opts.file.as_deref().map(str::trim).filter(|f| !f.is_empty());
    let found = project_map::users(&read(Need::Declarations { file, name: &name })?, file, &name)?;
    let mut any_suspect = false;
    let (mut any_route, mut any_route_suspect) = (false, false);
    let declarations: Vec<Value> = found
        .iter()
        .map(|d| {
            let (proven, by_candidates) = split_uses(&d.used_by);
            let mut entry = json!({
                "file": d.file,
                "name": d.name,
                "kind": d.kind,
                "line": d.line,
                "end_line": d.end_line,
                "used_by": proven,
            });
            if !by_candidates.is_empty() {
                any_suspect = true;
                entry["suspect"] = by_candidates
                    .into_iter()
                    .map(|(candidates, used_by)| json!({"candidates": candidates, "used_by": used_by}))
                    .collect();
            }
            if d.common_calls > 0 {
                entry["common_calls"] = json!(d.common_calls);
                let key = if d.common_calls == 1 { "map.users.common.one" } else { "map.users.common.many" };
                entry["common"] = json!(mustard_core::translate(key, lang)
                    .replace("{count}", &d.common_calls.to_string())
                    .replace("{name}", &d.name));
            }
            if !d.routes.is_empty() {
                any_route = true;
                entry["routes"] = d
                    .routes
                    .iter()
                    .map(|r| {
                        let (screens, by_candidates) = split_uses(&r.called_by);
                        let mut route = json!({
                            "method": r.method, "path": r.path, "file": d.file, "line": r.line, "screens": screens,
                        });
                        if !by_candidates.is_empty() {
                            any_route_suspect = true;
                            route["suspect"] = by_candidates
                                .into_iter()
                                .map(|(candidates, screens)| json!({"candidates": candidates, "screens": screens}))
                                .collect();
                        }
                        route
                    })
                    .collect();
            }
            let screens = d.routes.iter().any(|r| !r.called_by.is_empty());
            if d.used_by.is_empty() && d.common_calls == 0 && !screens {
                entry["note"] = json!(mustard_core::translate("map.users.none", lang)
                    .replace("{name}", &d.name)
                    .replace("{file}", &d.file));
            }
            entry
        })
        .collect();
    let mut report = json!({
        "ok": true,
        "question": "users",
        "name": name.trim(),
        "head": mustard_core::translate("map.users.head", lang).replace("{name}", name.trim()),
        "declarations": declarations,
    });
    if any_suspect {
        report["next"] = json!(mustard_core::translate("map.users.suspect", lang));
    }
    if any_route {
        report["route_head"] = json!(mustard_core::translate("map.users.routes", lang).replace("{name}", name.trim()));
    }
    if any_route_suspect {
        report["route_next"] = json!(mustard_core::translate("map.users.route_suspect", lang));
    }
    if let Some(file) = file {
        report["file"] = json!(project_map::clean_path(file));
    }
    if let Some(warning) = crate::commands::scan::ceiling_warning(root, opts.session.as_deref()) {
        report["warning"] = json!(warning);
    }
    Ok(report)
}

/// Os usos provados, como `arquivo:linha:quem chama`, e os suspeitos,
/// agrupados pelas declarações que a chamada pode alcançar.
fn split_uses(uses: &[UseSite]) -> (Vec<String>, BTreeMap<&[DeclAt], Vec<String>>) {
    let proven: Vec<String> = uses.iter().filter(|u| u.is_proven()).map(UseSite::place).collect();
    let mut by_candidates: BTreeMap<&[DeclAt], Vec<String>> = BTreeMap::new();
    for u in uses.iter().filter(|u| !u.is_proven()) {
        by_candidates.entry(u.candidates.as_slice()).or_default().push(u.place());
    }
    (proven, by_candidates)
}

/// Os exemplos para o alvo de `--file`; sem ele, para a pasta do arquivo que
/// a busca acha para `--task`, nas línguas `languages`. A receita do arquivo
/// cujo último commit ficou fora da janela do mapa sai da história dele,
/// lida por `trace` do git local na hora e gravada no mapa, como no pedido
/// da onda; a pergunta seguinte a lê do mapa. Como no pedido também, o
/// arquivo que mudou junto e não existe mais sai da receita, e a receita que
/// fica sem nada a dizer não sai. A história segue o número de
/// `map.historyMoves` lido como a pergunta da história o lê, com o mesmo
/// aviso do valor inválido.
fn examples(
    opts: &MapOpts,
    root: &Path,
    lang: Locale,
    languages: &Languages,
    read: &Reader<'_>,
    trace: &Trace<'_>,
) -> Result<Value, MapRefusal> {
    let file = opts.file.as_deref().map(str::trim).filter(|f| !f.is_empty());
    let task = opts.task.as_deref().map(str::trim).filter(|t| !t.is_empty());
    // A pasta que a tarefa acha sai dos nomes que cada arquivo declara; o
    // alvo dado pelo caminho não precisa deles. Sem nenhum dos dois, só as
    // recusas do mapa vêm antes da que diz o que falta.
    let need = match (file, task) {
        (None, None) => Need::Nothing,
        (file, _) => Need::Examples { words: file.is_none() },
    };
    let mut map = read(need)?;
    let target = match (file, task) {
        (Some(file), _) => project_map::clean_path(file),
        (None, Some(task)) => match project_map::best_folder(&map, task, languages) {
            Some(folder) => folder,
            None => {
                return Ok(json!({
                    "ok": true,
                    "question": "examples",
                    "task": task,
                    "examples": [],
                    "recipe": null,
                    "note": mustard_core::translate("map.no_target", lang),
                }));
            }
        },
        (None, None) => {
            return Err(MapRefusal::MissingArgument { question: "examples".to_string(), flag: "--file".to_string() });
        }
    };
    // O padrão que filtra os exemplos dá o papel pelo subprojeto também,
    // como no pedido da onda: os subprojetos vêm do terreno.
    map.projects = read(Need::Terrain).map(|terrain| terrain.projects).unwrap_or_default();
    let mut got = project_map::examples(&map, &target, lang);
    let mut warnings: Vec<String> = Vec::new();
    if got.recipe.is_none() && map.module(&target).is_some() {
        let config = mustard_core::ProjectConfig::load(root);
        let moves = history_moves(root, opts.session.as_deref(), lang, &config, &mut warnings);
        let model = store::model_path(root);
        let traced = |file: &str, moves: usize| trace(root, &model, file, moves).is_ok();
        got.recipe = history_beyond_window(read, &traced, &map.history, &target, moves)
            .as_ref()
            .and_then(project_map::recipe_from_lineage);
    }
    got.recipe = got.recipe.take().and_then(|recipe| recipe_on_disk(root, recipe));
    let picks: Vec<Value> = got
        .picks
        .iter()
        .map(|p| {
            json!({
                "path": p.path,
                "loc": p.loc,
                "why": p.why,
                "shared_imports": p.shared_imports,
                "tests": p.tests,
                "inline_tests": p.inline_tests,
                "last_change": p.last_change,
            })
        })
        .collect();
    // A receita do git: o trabalho que os commits contados fizeram — criar
    // um arquivo do tipo, pelo molde, ou mudar o arquivo —, quantos são, e o
    // que mudou junto em mais da metade deles.
    let recipe = got.recipe.as_ref().map(|r| {
        let (kind, subject) = match &r.of {
            project_map::RecipeOf::Created(pattern) => ("created", pattern),
            project_map::RecipeOf::Changed(path) => ("changed", path),
        };
        let together: Vec<Value> = r.together.iter().map(|(path, n)| json!({ "path": path, "commits": n })).collect();
        json!({ "kind": kind, "subject": subject, "commits": r.commits, "together": together, "tests": r.tests })
    });
    let mut report = json!({
        "ok": true,
        "question": "examples",
        "target": target,
        "folder": got.folder,
        "main_imports": got.main_imports,
        "examples": picks,
        "recipe": recipe,
    });
    if got.picks.is_empty() {
        report["note"] = json!(mustard_core::translate("map.no_examples", lang).replace("{folder}", &got.folder));
    }
    if let Some(why) = got.no_history {
        report["no_history"] = json!(why);
    }
    if !warnings.is_empty() {
        report["warnings"] = json!(warnings);
    }
    Ok(report)
}

/// Os arquivos que o mapa sugere para uma tarefa descrita em palavras, do
/// mais forte para o menos forte, pelo índice de busca do mapa. Vazio quando
/// não há mapa gravado ou quando nada casa: quem pergunta decide o que fazer
/// com a lista, porque o mapa não preenche a tarefa sozinho. A busca corta as
/// palavras nas línguas `languages`.
pub(crate) fn suggested_files(root: &Path, task: &str, limit: usize, languages: &Languages) -> Vec<String> {
    map_search::search(root, task, languages, limit).unwrap_or_default().into_iter().map(|found| found.path).collect()
}

/// A pasta que a skill descreve: a de cima do `.claude` onde ela mora.
fn skill_owner(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|a| a.file_name().is_some_and(|n| n == ".claude")).and_then(Path::parent).map(Path::to_path_buf)
}

/// Confere a skill de `--path`: cada caminho citado existe (na raiz do
/// projeto, na pasta da skill ou no mapa) e o texto cabe no limite.
fn skill(opts: &MapOpts, root: &Path, read: &Reader<'_>) -> Result<Value, MapRefusal> {
    let given = opts.path.as_deref().ok_or_else(|| MapRefusal::MissingArgument {
        question: "skill".to_string(),
        flag: "--path".to_string(),
    })?;
    let path = std::path::absolute(given).unwrap_or_else(|_| given.to_path_buf());
    let shown = given.to_string_lossy().replace('\\', "/");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| MapRefusal::SkillUnreadable { path: shown.clone(), detail: e.to_string() })?;
    let map = read(Need::Paths).ok();
    let owner = skill_owner(&path);
    let exists = |cited: &str| {
        root.join(cited).exists()
            || owner.as_ref().is_some_and(|o| o.join(cited).exists())
            || map.as_ref().is_some_and(|m| project_map::map_knows(m, cited))
    };
    project_map::check_skill(&text, exists)?;
    Ok(json!({
        "ok": true,
        "question": "skill",
        "path": shown,
        "lines": text.lines().count(),
        "cited": project_map::cited_paths(&text),
    }))
}

/// A história de cada declaração de `--name` na branch de partida (só a do
/// arquivo de `--file`, quando ele vem): o arquivo e a linha, quantas
/// mudanças a base tem dela fora as só de forma e os commits mais novos, do
/// mais novo ao mais velho, cada um com a data, o começo do hash, o título
/// limpo e o número do pull request; na última linha, o `git show` do mais
/// novo. Nunca o diff nem o código antigo. A lista do arquivo gravada no mapa
/// vale enquanto a base, a marca do scan e o commit mais novo do arquivo na
/// história guardada são os de quando ela se montou; vencida ou ausente,
/// `trace` a monta de novo, e só ela. Sem base, a resposta diz como
/// declarar; com o nome em mais de um arquivo e sem `--file`, lista os
/// lugares e pede o arquivo, sem montar nada.
/// A história: a de uma declaração, com `--name`, e o texto de um pull
/// request, com `--pr`; as duas juntas, ou só a do pull request.
fn history(opts: &MapOpts, root: &Path, lang: Locale, read: &Reader<'_>, trace: &Trace<'_>) -> Result<Value, MapRefusal> {
    let Some(number) = opts.pr else {
        return name_history(opts, root, lang, read, trace);
    };
    let pull = pull_answer(number, lang, read)?;
    if opts.name.as_deref().is_none_or(|name| name.trim().is_empty()) {
        return Ok(json!({ "ok": true, "question": "history", "pull": pull }));
    }
    let mut report = name_history(opts, root, lang, read, trace)?;
    report["pull"] = pull;
    Ok(report)
}

/// O título e o primeiro parágrafo da descrição do pull request `number`, ou
/// o aviso de que o mapa ainda não os tem.
fn pull_answer(number: u32, lang: Locale, read: &Reader<'_>) -> Result<Value, MapRefusal> {
    let map = read(Need::Pull(number))?;
    Ok(match project_map::pull_description(&map, number) {
        Some((title, description)) => json!({ "number": number, "title": title, "description": description }),
        None => json!({
            "number": number,
            "note": mustard_core::translate("map.history.pull_missing", lang).replace("{number}", &number.to_string()),
        }),
    })
}

fn name_history(opts: &MapOpts, root: &Path, lang: Locale, read: &Reader<'_>, trace: &Trace<'_>) -> Result<Value, MapRefusal> {
    let name = after_the_map(required(opts.name.as_deref(), opts.question, "--name"), read)?;
    let file = opts.file.as_deref().map(str::trim).filter(|f| !f.is_empty());
    let map = read(Need::History { file, name: &name })?;
    let files = project_map::declaring_files(&map, file, &name)?;
    let config = mustard_core::ProjectConfig::load(root);
    if let Some(note) = project_map::history_missing(&map.history, lang) {
        return Ok(json!({ "ok": true, "question": "history", "name": name, "note": note }));
    }
    let [path] = files.as_slice() else {
        let mut places: Vec<(&str, u64)> = map
            .modules
            .iter()
            .flat_map(|m| m.declarations.iter().filter(|d| d.name == name).map(move |d| (m.path.as_str(), d.line)))
            .collect();
        places.sort_unstable();
        return Ok(json!({
            "ok": true,
            "question": "history",
            "name": name,
            "places": places.iter().map(|(file, line)| format!("{file}:{line}")).collect::<Vec<_>>(),
            "note": mustard_core::translate("map.history.pick_file", lang).replace("{name}", &name),
        }));
    };
    let session = opts.session.as_deref();
    let mut warnings: Vec<String> = Vec::new();
    let moves = history_moves(root, session, lang, &config, &mut warnings);
    let commits = ("historyCommits", config.history_commits());
    let shown = history_number(root, session, lang, commits, project_map::DECL_COMMITS_SHOWN, &mut warnings);
    let calls = ("pullRequestCalls", config.pull_request_calls());
    history_number(root, session, lang, calls, crate::shared::pr_history::CALLS_PER_PASS, &mut warnings);
    let fresh = map
        .lineage
        .iter()
        .find(|lineage| &lineage.path == path)
        .is_some_and(|lineage| project_map::lineage_is_fresh(lineage, &map, moves));
    let map = if fresh {
        map
    } else {
        trace(root, &store::model_path(root), path, moves)
            .map_err(|e| MapRefusal::HistoryUnreadable { file: path.clone(), detail: e.to_string() })?;
        read(Need::History { file: Some(path), name: &name })?
    };
    let found = project_map::decl_history(&map, path, &name, shown)?;
    let base = map.history.base.as_str();
    let declarations: Vec<Value> = found
        .iter()
        .map(|d| {
            if d.commits.is_empty() {
                return json!({
                    "file": d.file,
                    "line": d.line,
                    "note": mustard_core::translate("map.history.not_in_base", lang)
                        .replace("{base}", base)
                        .replace("{name}", &name)
                        .replace("{file}", &d.file),
                });
            }
            let mut entry = json!({
                "file": d.file,
                "line": d.line,
                "changes": d.changes,
                "head": mustard_core::translate("map.history.head", lang)
                    .replace("{name}", &name)
                    .replace("{file}", &d.file)
                    .replace("{line}", &d.line.to_string())
                    .replace("{base}", base)
                    .replace("{count}", &d.changes.to_string()),
                "commits": d.commits.iter().map(|c| project_map::history_line(c, lang)).collect::<Vec<_>>(),
            });
            // O título de cada pull request uma vez, e os comentários de
            // revisão presos às linhas da declaração.
            let pulls: Vec<String> =
                d.pulls.iter().filter(|(_, title)| !title.is_empty()).map(|(number, title)| format!("#{number} {title}")).collect();
            if !pulls.is_empty() {
                entry["pulls"] = json!(pulls);
            }
            if !d.comments.is_empty() {
                entry["comments"] = json!(d.comments.iter().map(|(number, body)| format!("#{number} {body}")).collect::<Vec<_>>());
            }
            entry
        })
        .collect();
    let mut report = json!({ "ok": true, "question": "history", "name": name, "base": base, "declarations": declarations });
    let newest = found.iter().filter_map(|d| d.commits.first()).max_by(|a, b| a.at.cmp(&b.at).then_with(|| b.id.cmp(&a.id)));
    if let Some(newest) = newest {
        report["next"] = json!(mustard_core::translate("map.history.next", lang).replace("{commit}", &newest.id));
    }
    if !warnings.is_empty() {
        report["warnings"] = json!(warnings);
    }
    Ok(report)
}

/// O número de `map.historyMoves`, quantas mudanças de arquivo seguidas a
/// história de uma declaração segue, lido por um caminho só pela pergunta da
/// história, pela busca e pelos exemplos: o aviso do valor inválido sai uma
/// vez por sessão, seja qual for a pergunta que o leu primeiro.
fn history_moves(
    root: &Path,
    session: Option<&str>,
    lang: Locale,
    config: &mustard_core::ProjectConfig,
    warnings: &mut Vec<String>,
) -> usize {
    let moves = ("historyMoves", config.history_moves());
    history_number(root, session, lang, moves, project_map::MOVES_FOLLOWED, warnings)
}

/// O número da chave `key` da seção `map`, escrito como `setting`: o valor
/// inválido cai no padrão `default` e deixa em `warnings` o aviso que diz a
/// chave e o padrão, uma vez por sessão.
fn history_number(
    root: &Path,
    session: Option<&str>,
    lang: Locale,
    (key, setting): (&str, Setting),
    default: usize,
    warnings: &mut Vec<String>,
) -> usize {
    if setting == Setting::Invalid && first_warning(root, session, key) {
        warnings.push(
            mustard_core::translate("map.history.bad_setting", lang)
                .replace("{key}", key)
                .replace("{default}", &default.to_string()),
        );
    }
    setting.or(default)
}

/// Se o aviso do valor inválido de `key` ainda não saiu na sessão `session`;
/// se não saiu, marca que saiu, em `.claude/.session/<sessão>/`. Sem sessão
/// conhecida, avisa sempre: repetir o aviso é melhor que calar o valor que
/// não vale.
pub(crate) fn first_warning(root: &Path, session: Option<&str>, key: &str) -> bool {
    let usable = |s: &&str| !s.is_empty() && *s != "unknown" && !s.starts_with('.') && !s.contains(['/', '\\']);
    let Some(session) = session.map(str::trim).filter(usable) else {
        return true;
    };
    let Ok(paths) = mustard_core::ClaudePaths::for_project(root) else { return true };
    let marker = paths.claude_dir().join(".session").join(session).join(format!("warned-map-{key}"));
    if marker.is_file() {
        return false;
    }
    if let Some(dir) = marker.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&marker, "");
    true
}

/// Imprime a resposta e sai com 1 na recusa.
pub fn run(opts: &MapOpts) {
    let scan = mustard_core::Scan::locate();
    let report = map_at(
        opts,
        &|root, out| scan.scan(root, out),
        &|root, out, file, moves| scan.history(root, out, file, moves),
        &jev,
    );
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::project_map::{
        DeclChange, DeclComment, DeclLineage, FileLineage, LineageCommit, PullComment, PullText,
    };
    use tempfile::tempdir;

    /// A resposta de um projeto sem a chave do filtro: a busca é a do banco.
    fn map_at(opts: &MapOpts, mine: &Mine<'_>, trace: &Trace<'_>) -> Value {
        super::map_at(opts, mine, trace, &|_, _| Err(FilterError::MissingKey))
    }

    /// A resposta do comando a um mapa de teste, que ninguém relê: fora do
    /// git, a conferência não chama o scan.
    fn answered(opts: &MapOpts) -> Value {
        map_at(opts, &|_, _| panic!("a map outside git is never read again"), &|_, _, _, _| {
            panic!("a map without a base never reads a history")
        })
    }

    /// Um mapa pequeno: uma pasta de comandos com quatro arquivos que
    /// importam o mesmo núcleo, um teste que cobre um deles e um histórico.
    const MODEL: &str = r#"{
      "modules": [
        {"path": "apps/rt/src/commands/pay/read.rs", "loc": 100, "declarations": [{"name": "run"}],
         "deps": ["packages/core/src/pay.rs"], "has_tests": true},
        {"path": "apps/rt/src/commands/pay/write.rs", "loc": 120, "declarations": [{"name": "run"}],
         "deps": ["packages/core/src/pay.rs"], "tests": ["apps/rt/tests/pay_cli.rs"]},
        {"path": "apps/rt/src/commands/pay/index.rs", "loc": 90, "declarations": [{"name": "run"}],
         "deps": ["packages/core/src/pay.rs"]},
        {"path": "apps/rt/src/commands/pay/cli.rs", "loc": 3000, "declarations": [{"name": "dispatch"}],
         "deps": ["packages/core/src/pay.rs"]},
        {"path": "packages/core/src/pay.rs", "loc": 200, "declarations": [{"name": "Payment"}]},
        {"path": "apps/rt/tests/pay_cli.rs", "loc": 80, "deps": ["apps/rt/src/commands/pay/write.rs"]}
      ],
      "history": {
        "paths": ["apps/rt/src/commands/pay/index.rs", "apps/rt/src/commands/pay/read.rs",
                  "apps/rt/src/commands/pay/refund.rs", "apps/rt/src/commands/pay/write.rs",
                  "apps/rt/tests/run_command_surface.rs"],
        "commits": [
          {"id": "c1", "at": 86400, "added": [0, 4]},
          {"id": "c2", "at": 172800, "added": [1], "changed": [4]},
          {"id": "c3", "at": 259200, "changed": [3]},
          {"id": "c4", "at": 345600, "added": [2], "changed": [4]}
        ]
      }
    }"#;

    fn project_with_map() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        store::write_text(dir.path(), MODEL).unwrap();
        dir
    }

    /// Um arquivo qualquer em `path`, na pasta do projeto `root`.
    fn touch(root: &Path, path: &str) {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, "pub fn run() {}\n").unwrap();
    }

    fn ask(root: &Path, question: Question) -> MapOpts {
        MapOpts {
            root: root.to_path_buf(),
            question,
            file: None,
            task: None,
            query: None,
            intent: None,
            path: None,
            name: None,
            pr: None,
            session: None,
        }
    }

    /// Num projeto que escreve em português e programa em inglês, a busca do
    /// mapa corta a pergunta também como inglês: "users" acha o arquivo que
    /// declara `UserRepository`. Com o código declarado em português, a mesma
    /// pergunta não acha nada, porque o português não tira o plural de
    /// "users".
    #[test]
    fn users_finds_user_repository_when_the_project_codes_in_english() {
        let dir = tempdir().unwrap();
        store::write_text(
            dir.path(),
            r#"{"modules": [
              {"path": "src/storage.rs", "loc": 40, "declarations": [{"name": "UserRepository"}]},
              {"path": "src/billing.rs", "loc": 40, "declarations": [{"name": "Payment"}]}
            ]}"#,
        )
        .unwrap();
        let config = dir.path().join("mustard.json");
        let mut opts = ask(dir.path(), Question::Search);
        opts.query = Some("users".to_string());

        std::fs::write(&config, r#"{"language": {"text": "pt-BR", "code": "en-US"}}"#).unwrap();
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["files"], json!([{"path": "src/storage.rs", "score": report["files"][0]["score"]}]), "{report}");

        std::fs::write(&config, r#"{"language": {"text": "pt-BR", "code": "pt-BR"}}"#).unwrap();
        let report = answered(&opts);
        assert_eq!(report["files"], json!([]), "{report}");
    }

    /// A resposta de quem usa, onde a contagem das chamadas comuns aparece,
    /// traz em `warning` o teto do nome comum escrito errado no
    /// `mustard.json`, com a chave, o valor lido e o padrão, uma vez na
    /// sessão; a mesma pergunta de novo na sessão não o repete.
    #[test]
    fn users_warns_once_per_session_about_an_invalid_common_name_ceiling() {
        let dir = tempdir().unwrap();
        store::write_text(
            dir.path(),
            r#"{"modules": [{"path": "src/m1.rs", "loc": 3, "declarations": [{"name": "comum", "common_calls": 1}]}]}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("mustard.json"), r#"{"scan": {"max_same_name": "dois"}}"#).unwrap();
        let opts = MapOpts { name: Some("comum".to_string()), session: Some("sessao".to_string()), ..ask(dir.path(), Question::Users) };

        let first = answered(&opts);
        assert_eq!(first["declarations"][0]["common_calls"], json!(1), "{first}");
        let warning = first["warning"].as_str().unwrap_or_else(|| panic!("sem aviso: {first}"));
        for part in ["scan.max_same_name", "\"dois\"", "8"] {
            assert!(warning.contains(part), "o aviso cita {part}: {warning}");
        }
        let again = answered(&opts);
        assert!(again.get("warning").is_none(), "um aviso por sessão: {again}");
    }

    /// O índice da busca guarda as línguas em que foi feito: quando o projeto
    /// passa a programar em inglês, a busca seguinte o refaz, e "processed"
    /// acha o arquivo que declara `Processing`, que o índice só em português
    /// não achava.
    #[test]
    fn a_change_in_the_project_languages_remakes_the_search_index() {
        let dir = tempdir().unwrap();
        store::write_text(
            dir.path(),
            r#"{"modules": [
              {"path": "src/queue.rs", "loc": 40, "declarations": [{"name": "Processing"}]},
              {"path": "src/billing.rs", "loc": 40, "declarations": [{"name": "Payment"}]}
            ]}"#,
        )
        .unwrap();
        let config = dir.path().join("mustard.json");
        let mut opts = ask(dir.path(), Question::Search);
        opts.query = Some("processed".to_string());

        std::fs::write(&config, r#"{"language": {"text": "pt-BR", "code": "pt-BR"}}"#).unwrap();
        let report = answered(&opts);
        assert_eq!(report["files"], json!([]), "{report}");

        std::fs::write(&config, r#"{"language": {"text": "pt-BR", "code": "en-US"}}"#).unwrap();
        let report = answered(&opts);
        assert_eq!(report["files"][0]["path"], json!("src/queue.rs"), "{report}");
    }

    /// Um mapa em que uma coluna que a busca não lê guarda o tipo errado:
    /// texto onde se espera a lista dos arquivos importados. O mapa inteiro
    /// não se lê, e a busca lê só o índice de palavras.
    const WRONG_TYPE: &str = r#"{"modules": [
      {"path": "src/pedido.rs", "loc": 40, "deps": "texto no lugar da lista", "declarations": [{"name": "buscarPedido"}]},
      {"path": "src/cliente.rs", "loc": 40, "declarations": [{"name": "Cliente"}]}
    ]}"#;

    #[test]
    fn the_search_question_answers_a_map_with_a_column_it_does_not_read_in_the_wrong_type() {
        let dir = tempdir().unwrap();
        store::write_text(dir.path(), WRONG_TYPE).unwrap();
        assert!(store::read(dir.path()).is_err(), "the whole map does not read");
        let mut opts = ask(dir.path(), Question::Search);
        opts.query = Some("buscar pedido".to_string());
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["files"][0]["path"], json!("src/pedido.rs"), "{report}");
    }

    #[test]
    fn suggested_files_come_from_a_map_with_a_column_they_do_not_read_in_the_wrong_type() {
        let dir = tempdir().unwrap();
        store::write_text(dir.path(), WRONG_TYPE).unwrap();
        assert!(store::read(dir.path()).is_err(), "the whole map does not read");
        let languages = Languages::new(["pt-BR", "en-US"]);
        assert_eq!(suggested_files(dir.path(), "buscar pedido", 3, &languages), ["src/pedido.rs"]);
    }

    #[test]
    fn a_task_asking_for_examples_gets_two_or_three_from_the_same_folder_with_the_reason() {
        let dir = project_with_map();
        // O registro de teste que a receita cita existe no projeto.
        touch(dir.path(), "apps/rt/tests/run_command_surface.rs");
        let mut opts = ask(dir.path(), Question::Examples);
        opts.task = Some("adicionar um comando run".to_string());
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["folder"], json!("apps/rt/src/commands/pay"));
        let picks = report["examples"].as_array().unwrap();
        assert!((2..=3).contains(&picks.len()), "{report}");
        for pick in picks {
            assert!(pick["path"].as_str().unwrap().starts_with("apps/rt/src/commands/pay/"), "{pick}");
            assert_eq!(pick["shared_imports"], json!(["packages/core/src/pay.rs"]), "{pick}");
            assert!(!pick["why"].as_array().unwrap().is_empty(), "{pick}");
            assert_ne!(pick["path"], json!("apps/rt/src/commands/pay/cli.rs"), "the oversized file is left out");
        }
        // Com teste e mais recente primeiro.
        assert_eq!(picks[0]["path"], json!("apps/rt/src/commands/pay/write.rs"));
        assert_eq!(picks[1]["path"], json!("apps/rt/src/commands/pay/read.rs"));
        // A receita soma os três commits que criaram um arquivo na pasta.
        assert_eq!(
            report["recipe"],
            json!({"kind": "created", "subject": "apps/rt/src/commands/pay/*", "commits": 3,
                   "together": [{"path": "apps/rt/tests/run_command_surface.rs", "commits": 3}], "tests": null}),
            "{report}"
        );
    }

    /// Um projeto com o mapa cuja janela está cheia com `MAX_COMMITS`
    /// commits que nunca tocam `src/old.rs`: o último commit dele ficou fora
    /// da janela.
    fn project_with_a_file_older_than_the_window() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let commits: Vec<Value> = (0..project_map::MAX_COMMITS)
            .map(|n| json!({"id": format!("n{n:05}"), "at": 1_789_000_000 + n, "changed": [0]}))
            .collect();
        let modules = json!([
            {"path": "src/old.rs", "loc": 30},
            {"path": "src/registry.rs", "loc": 30},
            {"path": "src/busy.rs", "loc": 30}
        ]);
        let map = json!({"modules": modules, "history": {"base": "main", "paths": ["src/busy.rs"], "commits": commits}});
        store::write_text(root, &map.to_string()).unwrap();
        for path in ["src/old.rs", "src/registry.rs", "src/busy.rs"] {
            touch(root, path);
        }
        dir
    }

    /// A passada falsa da história: grava no mapa em `out` a de `file`,
    /// seguindo `moves` mudanças, como a de verdade: três commits que mudaram
    /// o registro junto com ele.
    fn three_commits_with_the_registry(
        out: &Path,
        file: &str,
        moves: usize,
    ) -> mustard_core::platform::error::Result<HistoryReport> {
        let mark = store::read_for_at(out, Need::Lineage(file)).unwrap().census_mark;
        let commit = |id: &str| LineageCommit {
            id: id.to_string(),
            files: project_map::CommitFiles {
                changed: vec![file.to_string(), "src/registry.rs".to_string()],
                ..Default::default()
            },
            ..LineageCommit::default()
        };
        let lineage = FileLineage {
            path: file.to_string(),
            base: "main".to_string(),
            mark,
            moves: u32::try_from(moves).unwrap(),
            commits: vec![commit("a1"), commit("a2"), commit("a3")],
            ..FileLineage::default()
        };
        store::save_lineage_at(out, &lineage)?;
        Ok(HistoryReport::default())
    }

    /// Os exemplos de um arquivo cujo último commit ficou fora da janela do
    /// mapa, cheia com `MAX_COMMITS` commits que nunca o tocam: a primeira
    /// pergunta lê do git a história dele, com três commits que mudaram o
    /// registro junto, e a receita sai dela; a segunda lê a história que
    /// ficou gravada no mapa, sem voltar ao git.
    #[test]
    fn examples_of_a_file_older_than_the_window_read_its_history_once_and_the_next_question_does_not_read_git() {
        let dir = project_with_a_file_older_than_the_window();
        let root = dir.path();
        let calls = std::cell::Cell::new(0);
        let trace = |_: &Path, out: &Path, file: &str, moves: usize| {
            calls.set(calls.get() + 1);
            three_commits_with_the_registry(out, file, moves)
        };
        let opts = MapOpts { file: Some("src/old.rs".to_string()), ..ask(root, Question::Examples) };
        for round in 0..2 {
            let report = map_at(&opts, &|_, _| panic!("a map outside git is never read again"), &trace);
            assert_eq!(
                report["recipe"],
                json!({"kind": "changed", "subject": "src/old.rs", "commits": 3,
                       "together": [{"path": "src/registry.rs", "commits": 3}], "tests": null}),
                "round {round}: {report}"
            );
        }
        assert_eq!(calls.get(), 1, "the second question reads the history kept in the map, not git");
    }

    /// Nos exemplos, o valor inválido de `map.historyMoves` — zero, negativo
    /// ou texto — cai no padrão e sai com o aviso que diz a chave e o padrão,
    /// uma vez só na sessão; outra sessão recebe o aviso de novo. Com o valor
    /// certo, a história segue o número escrito, sem aviso.
    #[test]
    fn an_invalid_history_moves_in_the_examples_falls_back_to_the_default_with_one_warning() {
        for bad in ["0", "-2", "\"dez\""] {
            let dir = project_with_a_file_older_than_the_window();
            let root = dir.path();
            let config = |text: String| std::fs::write(root.join("mustard.json"), text).unwrap();
            config(format!(r#"{{"map": {{"historyMoves": {bad}}}}}"#));
            let moved = std::cell::RefCell::new(Vec::new());
            let trace = |_: &Path, out: &Path, file: &str, moves: usize| {
                moved.borrow_mut().push(moves);
                three_commits_with_the_registry(out, file, moves)
            };
            let ask_in = |session: &str| {
                let opts = MapOpts {
                    file: Some("src/old.rs".to_string()),
                    session: Some(session.to_string()),
                    ..ask(root, Question::Examples)
                };
                map_at(&opts, &|_, _| panic!("a map outside git is never read again"), &trace)
            };
            let report = ask_in("sessao-1");
            assert_eq!(*moved.borrow(), vec![project_map::MOVES_FOLLOWED], "{bad}: {report}");
            assert_eq!(report["recipe"]["commits"], json!(3), "{bad}: {report}");
            let warnings: Vec<&str> =
                report["warnings"].as_array().map(|all| all.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            assert_eq!(warnings.len(), 1, "{bad}: {report}");
            assert!(warnings[0].contains("map.historyMoves"), "{bad}: {report}");
            assert!(warnings[0].contains(&project_map::MOVES_FOLLOWED.to_string()), "{bad}: {report}");
            let again = ask_in("sessao-1");
            assert!(again.get("warnings").is_none(), "{bad}: one warning per session: {again}");
            assert!(ask_in("sessao-2").get("warnings").is_some(), "{bad}: another session is warned again");

            config(r#"{"map": {"historyMoves": 2}}"#.to_string());
            let valid = ask_in("sessao-3");
            assert!(valid.get("warnings").is_none(), "{bad}: {valid}");
            assert_eq!(moved.borrow().last(), Some(&2), "{bad}: {valid}");
        }
    }

    /// Três commits criaram um arquivo em `src/cmd` e mudaram junto o índice
    /// da pasta e `src/gone.rs`, que não existe mais no projeto: a receita do
    /// arquivo novo cita só o índice. Com o índice apagado também, ela fica
    /// sem nada a dizer e não sai.
    #[test]
    fn the_examples_recipe_leaves_out_a_file_that_no_longer_exists_and_is_not_shown_when_nothing_is_left() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let map = json!({
            "modules": [
                {"path": "src/cmd/a.rs", "loc": 30}, {"path": "src/cmd/b.rs", "loc": 30},
                {"path": "src/cmd/c.rs", "loc": 30}, {"path": "src/cmd/mod.rs", "loc": 30}
            ],
            "history": {
                "paths": ["src/cmd/a.rs", "src/cmd/b.rs", "src/cmd/c.rs", "src/cmd/mod.rs", "src/gone.rs"],
                "commits": [
                    {"id": "c1", "at": 86_400, "added": [0], "changed": [3, 4]},
                    {"id": "c2", "at": 172_800, "added": [1], "changed": [3, 4]},
                    {"id": "c3", "at": 259_200, "added": [2], "changed": [3, 4]}
                ]
            }
        });
        store::write_text(root, &map.to_string()).unwrap();
        for path in ["src/cmd/a.rs", "src/cmd/b.rs", "src/cmd/c.rs", "src/cmd/mod.rs"] {
            touch(root, path);
        }
        let opts = MapOpts { file: Some("src/cmd/novo.rs".to_string()), ..ask(root, Question::Examples) };
        let report = answered(&opts);
        assert_eq!(
            report["recipe"],
            json!({"kind": "created", "subject": "src/cmd/*.rs", "commits": 3,
                   "together": [{"path": "src/cmd/mod.rs", "commits": 3}], "tests": null}),
            "{report}"
        );
        std::fs::remove_file(root.join("src/cmd/mod.rs")).unwrap();
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["recipe"], Value::Null, "{report}");
    }

    /// A busca acha o arquivo pela mensagem que o usuário viu, e mostra o
    /// texto que casou com a linha e a declaração onde ele nasce.
    #[test]
    fn the_search_answer_shows_the_matched_text_with_its_line_and_declaration() {
        let dir = tempdir().unwrap();
        store::write_text(
            dir.path(),
            r#"{"modules": [
              {"path": "src/consulta.rs", "loc": 40, "declarations": [{"name": "carregar", "line": 1, "end_line": 9}],
               "texts": [{"line": 2, "kind": "log", "value": "carregando o registro", "owner": "carregar"},
                         {"line": 4, "kind": "error", "value": "pedido não encontrado", "owner": "carregar"}]},
              {"path": "src/billing.rs", "loc": 40, "declarations": [{"name": "Payment"}]}
            ]}"#,
        )
        .unwrap();
        let mut opts = ask(dir.path(), Question::Search);
        opts.query = Some("pedido não encontrado".to_string());
        let report = answered(&opts);
        assert_eq!(report["files"][0]["path"], json!("src/consulta.rs"), "{report}");
        assert_eq!(
            report["files"][0]["text"],
            json!({"line": 4, "kind": "error", "value": "pedido não encontrado", "owner": "carregar"}),
            "{report}"
        );
    }

    #[test]
    fn importers_tests_search_and_summary_answer_from_the_map() {
        let dir = project_with_map();
        let mut opts = ask(dir.path(), Question::Importers);
        opts.file = Some("packages/core/src/pay.rs".to_string());
        let report = answered(&opts);
        assert_eq!(report["importers"].as_array().unwrap().len(), 4, "{report}");

        let mut opts = ask(dir.path(), Question::Tests);
        opts.file = Some("apps/rt/src/commands/pay/write.rs".to_string());
        assert_eq!(answered(&opts)["tests"], json!(["apps/rt/tests/pay_cli.rs"]));

        let mut opts = ask(dir.path(), Question::Search);
        opts.query = Some("pagamento".to_string());
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");

        let report = answered(&ask(dir.path(), Question::Summary));
        assert!(report["bytes"].as_u64().unwrap() <= 3 * 1024, "{report}");
    }

    #[test]
    fn a_question_without_its_option_or_without_a_map_is_refused_by_name() {
        let dir = tempdir().unwrap();
        let report = answered(&ask(dir.path(), Question::Summary));
        assert_eq!(report["reason"], json!("map-missing"));
        assert!(report["hint"].as_str().unwrap().contains("mustard-rt run scan"));

        let dir = project_with_map();
        let report = answered(&ask(dir.path(), Question::Importers));
        assert_eq!(report["reason"], json!("missing-argument"));
        assert!(report["hint"].as_str().unwrap().contains("--file"));

        let mut opts = ask(dir.path(), Question::Tests);
        opts.file = Some("nao/existe.rs".to_string());
        assert_eq!(answered(&opts)["reason"], json!("unknown-file"));
    }

    /// O trecho de uma declaração vem do mapa mais o arquivo: quem pergunta
    /// não abre o arquivo, e recebe as linhas da declaração, do começo ao fim,
    /// com o caminho e as linhas de onde saíram.
    #[test]
    fn o_mapa_devolve_o_trecho_de_uma_declaracao() {
        let dir = tempdir().unwrap();
        let file = "packages/core/src/pay.rs";
        std::fs::create_dir_all(dir.path().join("packages/core/src")).unwrap();
        std::fs::write(
            dir.path().join(file),
            "// topo do arquivo\n\
             /// Soma o preço do pedido com o frete.\n\
             pub fn total(preco: u32, frete: u32) -> u32 {\n    \
                 preco + frete\n\
             }\n\
             // depois\n",
        )
        .unwrap();
        store::write_text(
            dir.path(),
            &format!(
                r#"{{"modules": [{{"path": "{file}", "loc": 6, "declarations": [
                     {{"kind": "function", "name": "total", "line": 3, "end_line": 5,
                      "doc": "Soma o preço do pedido com o frete.",
                      "signature": "pub fn total(preco: u32, frete: u32) -> u32"}}]}}]}}"#
            ),
        )
        .unwrap();

        let mut opts = ask(dir.path(), Question::Slice);
        opts.file = Some(file.to_string());
        opts.name = Some("total".to_string());
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(
            report["slice"],
            json!("pub fn total(preco: u32, frete: u32) -> u32 {\n    preco + frete\n}"),
            "da primeira à última linha da declaração, e nada em volta: {report}"
        );
        assert_eq!(report["file"], json!(file), "{report}");
        assert_eq!(report["line"], json!(3), "{report}");
        assert_eq!(report["end_line"], json!(5), "{report}");
        assert_eq!(report["name"], json!("total"), "{report}");
        assert_eq!(report["doc"], json!("Soma o preço do pedido com o frete."), "{report}");
        assert_eq!(report["signature"], json!("pub fn total(preco: u32, frete: u32) -> u32"), "{report}");

        // Sem o nome da declaração, e com um nome que o arquivo não declara,
        // a recusa diz qual é o caso.
        let mut sem_nome = ask(dir.path(), Question::Slice);
        sem_nome.file = Some(file.to_string());
        let report = answered(&sem_nome);
        assert_eq!(report["reason"], json!("missing-argument"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("--name"), "{report}");

        opts.name = Some("sumiu".to_string());
        let report = answered(&opts);
        assert_eq!(report["reason"], json!("unknown-declaration"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("sumiu"), "{report}");
    }

    /// A função do controlador que atende uma rota traz, na resposta de quem
    /// a usa, a rota com o arquivo e a linha dela e as chamadas da tela que a
    /// alcançam, cada uma com o arquivo e a linha: a provada, e a suspeita com
    /// a função que ela pode alcançar. A rota que outra função atende não
    /// entra.
    #[test]
    fn users_of_a_controller_function_shows_the_screen_calls_with_their_lines() {
        let dir = tempdir().unwrap();
        store::write_text(
            dir.path(),
            r#"{"modules": [
              {"path": "servidor/src/pedidos.controller.ts", "language": "typescript", "loc": 12,
               "declarations": [{"kind": "method", "name": "editar", "line": 5, "end_line": 7},
                                {"kind": "method", "name": "listar", "line": 9, "end_line": 10}],
               "routes": [
                 {"method": "POST", "path": "api/v1/pedidos/edit", "written": "api/v1/pedidos/edit", "handler": "editar",
                  "line": 5, "framework": "nestjs",
                  "called_by": ["tela/src/pedidos.ts:6:salvar",
                                {"at": "tela/src/planos.ts:4:enviar",
                                 "candidates": ["servidor/src/pedidos.controller.ts:5:editar"]}]},
                 {"method": "GET", "path": "api/v1/pedidos", "written": "api/v1/pedidos", "handler": "listar", "line": 9,
                  "framework": "nestjs", "called_by": ["tela/src/lista.ts:3:abrir"]}]},
              {"path": "tela/src/pedidos.ts", "language": "typescript", "loc": 8,
               "declarations": [{"kind": "function", "name": "salvar", "line": 5}]}
            ]}"#,
        )
        .unwrap();
        let mut opts = ask(dir.path(), Question::Users);
        opts.name = Some("editar".to_string());
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        let found = &report["declarations"][0];
        assert_eq!(
            found["routes"],
            json!([{
                "method": "POST", "path": "api/v1/pedidos/edit", "file": "servidor/src/pedidos.controller.ts", "line": 5,
                "screens": ["tela/src/pedidos.ts:6:salvar"],
                "suspect": [{"candidates": ["servidor/src/pedidos.controller.ts:5:editar"],
                             "screens": ["tela/src/planos.ts:4:enviar"]}]
            }]),
            "{report}"
        );
        assert_eq!(found.get("note"), None, "a function reached from the screen is used: {report}");
        assert!(report["route_head"].as_str().unwrap().contains("editar"), "{report}");
        assert!(report["route_next"].is_string(), "{report}");
    }

    /// Um mapa com tudo o que as perguntas leem: línguas, subprojetos, os
    /// mais importados, a história, um arquivo escrito por máquina, o mesmo
    /// nome declarado em vários arquivos (fora da ordem dos caminhos) e duas
    /// vezes no mesmo, e os usos de cada declaração.
    const EVERY_PART: &str = r#"{
      "modules": [
        {"path": "apps/rt/src/commands/pay/write.rs", "language": "rust", "loc": 120,
         "declarations": [
           {"kind": "function", "name": "run", "line": 2, "end_line": 4, "doc": "Grava.", "signature": "pub fn run()",
            "used_by": ["apps/rt/src/main.rs:9:main"]},
           {"kind": "function", "name": "run", "line": 6, "end_line": 7}
         ],
         "deps": ["packages/core/src/pay.rs"], "tests": ["apps/rt/tests/pay_cli.rs"]},
        {"path": "apps/rt/src/commands/pay/read.rs", "language": "rust", "loc": 100, "has_tests": true,
         "declarations": [{"kind": "function", "name": "run", "line": 1, "end_line": 3}],
         "deps": ["packages/core/src/pay.rs"]},
        {"path": "apps/rt/src/commands/pay/index.rs", "language": "rust", "loc": 90,
         "declarations": [{"kind": "function", "name": "run", "line": 5}, {"kind": "struct", "name": "Index", "line": 1}],
         "deps": ["packages/core/src/pay.rs", "apps/rt/src/commands/pay/read.rs"]},
        {"path": "apps/rt/src/commands/pay/cli.rs", "language": "rust", "loc": 3000,
         "declarations": [{"name": "dispatch", "line": 1}], "deps": ["packages/core/src/pay.rs"]},
        {"path": "apps/rt/src/commands/pay/gen.rs", "language": "rust", "loc": 95, "file_class": "generated",
         "declarations": [{"name": "run", "line": 1}], "deps": ["packages/core/src/pay.rs"]},
        {"path": "packages/core/src/pay.rs", "language": "rust", "loc": 200,
         "declarations": [{"kind": "struct", "name": "Payment", "line": 1, "end_line": 3, "used_by": ["a.rs:1"]}]},
        {"path": "apps/rt/tests/pay_cli.rs", "language": "rust", "loc": 80, "deps": ["apps/rt/src/commands/pay/write.rs"]},
        {"path": "web/src/pay.ts", "language": "typescript", "loc": 60, "declarations": [{"name": "run", "line": 3}],
         "routes": [{"method": "GET", "path": "pay", "written": "/pay", "handler": "run", "line": 3, "framework": "express",
                     "called_by": ["web/src/tela.ts:2:abrir"]}]}
      ],
      "projects": [
        {"name": "rt", "dir": "apps/rt", "kind": "cargo", "code_files": 6},
        {"name": "core", "dir": "packages/core", "kind": "cargo", "code_files": 1},
        {"name": "web", "dir": "web", "kind": "npm", "code_files": 1}
      ],
      "languages": [{"language": "rust", "files": 7, "loc": 3595}, {"language": "typescript", "files": 1, "loc": 60}],
      "graph": {"top_fan_in": [{"module": "packages/core/src/pay.rs", "degree": 5}, {"module": "apps/rt/src/commands/pay/read.rs", "degree": 1}]},
      "skeleton": [{"dir": "apps/rt", "role": "L1"}, {"dir": "packages/core", "role": "L0"}],
      "history": {
        "paths": ["apps/rt/src/commands/pay/index.rs", "apps/rt/src/commands/pay/read.rs",
                  "apps/rt/src/commands/pay/write.rs", "web/src/pay.ts"],
        "commits": [
          {"id": "c1", "at": 86400, "added": [0, 3]},
          {"id": "c2", "at": 172800, "added": [1], "changed": [3]},
          {"id": "c3", "at": 259200, "changed": [2]}
        ]
      }
    }"#;

    /// Cada pergunta, com as opções dela: as que acham, as que não acham e
    /// as que faltam.
    fn every_question(root: &Path, skill: &Path) -> Vec<MapOpts> {
        let with = |question: Question, file: Option<&str>, name: Option<&str>, task: Option<&str>| MapOpts {
            file: file.map(str::to_string),
            name: name.map(str::to_string),
            task: task.map(str::to_string),
            ..ask(root, question)
        };
        let mut skill_opts = ask(root, Question::Skill);
        skill_opts.path = Some(skill.to_path_buf());
        vec![
            ask(root, Question::Summary),
            with(Question::Importers, Some("packages/core/src/pay.rs"), None, None),
            with(Question::Importers, Some("./apps/rt/src/commands/pay/read.rs"), None, None),
            with(Question::Importers, Some("web/src/pay.ts"), None, None),
            with(Question::Importers, Some("nao/existe.rs"), None, None),
            with(Question::Importers, None, None, None),
            with(Question::Tests, Some("apps/rt/src/commands/pay/write.rs"), None, None),
            with(Question::Tests, Some("apps/rt/src/commands/pay/read.rs"), None, None),
            with(Question::Tests, Some("nao/existe.rs"), None, None),
            with(Question::Tests, None, None, None),
            with(Question::Slice, Some("apps/rt/src/commands/pay/write.rs"), Some(" run "), None),
            with(Question::Slice, Some("apps/rt/src/commands/pay/write.rs"), Some("sumiu"), None),
            with(Question::Slice, Some("nao/existe.rs"), Some("run"), None),
            with(Question::Slice, Some("apps/rt/src/commands/pay/write.rs"), None, None),
            with(Question::Slice, None, Some("run"), None),
            with(Question::Users, None, Some("run"), None),
            with(Question::Users, Some("apps/rt/src/commands/pay/write.rs"), Some("run"), None),
            with(Question::Users, Some("packages/core/src/pay.rs"), Some("Payment"), None),
            with(Question::Users, Some("packages/core/src/pay.rs"), Some("run"), None),
            with(Question::Users, Some("web/src/pay.ts"), Some("run"), None),
            with(Question::Users, Some("nao/existe.rs"), Some("run"), None),
            with(Question::Users, None, Some("sumiu"), None),
            with(Question::Users, None, None, None),
            with(Question::History, Some("apps/rt/src/commands/pay/write.rs"), Some("run"), None),
            with(Question::History, None, Some("run"), None),
            with(Question::History, None, Some("Payment"), None),
            with(Question::History, None, Some("sumiu"), None),
            with(Question::History, Some("nao/existe.rs"), Some("run"), None),
            with(Question::History, None, None, None),
            with(Question::Examples, Some("apps/rt/src/commands/pay/novo.rs"), None, None),
            with(Question::Examples, Some("apps/rt/src/commands/pay/index.rs"), None, None),
            with(Question::Examples, Some("apps/rt/src/commands/pay"), None, None),
            with(Question::Examples, None, None, Some("adicionar um comando run")),
            with(Question::Examples, None, None, Some("nada casa com isto")),
            with(Question::Examples, None, None, None),
            skill_opts,
        ]
    }

    /// A resposta de cada pergunta com o mapa inteiro lido, como era antes
    /// de cada pergunta ler só as tabelas dela.
    fn answered_from_the_whole_map(opts: &MapOpts) -> Value {
        let project = crate::commands::spec_events::project(&opts.root);
        let whole = |_: Need<'_>| store::read(&project.root);
        let trace = |_: &Path, _: &Path, _: &str, _: usize| panic!("a map without a base never reads a history");
        answer_from(opts, &project.root, project.lang, &project.languages, &whole, &trace, &|_, _| Err(FilterError::MissingKey))
            .unwrap_or_else(|refusal| refused(&refusal, project.lang))
    }

    /// Cada pergunta lê só as tabelas dela e responde o mesmo que respondia
    /// com o mapa inteiro, byte a byte: com o mapa cheio, e sem mapa.
    #[test]
    fn every_question_reading_only_its_tables_answers_as_the_whole_map_did() {
        let dir = tempdir().unwrap();
        store::write_text(dir.path(), EVERY_PART).unwrap();
        let file = dir.path().join("apps/rt/src/commands/pay/write.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "// topo\npub fn run() {\n    gravar();\n}\n\nfn run() {}\n").unwrap();
        let skill_dir = dir.path().join("apps/rt/.claude/skills/add-pay");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill = skill_dir.join("SKILL.md");
        std::fs::write(&skill, "Veja `pay/index.rs`, `commands/pay/` e `apps/rt/src/commands/pay/sumiu.rs`.\n").unwrap();

        let mut found = 0;
        for opts in every_question(dir.path(), &skill) {
            let now = answered(&opts);
            assert_eq!(now, answered_from_the_whole_map(&opts), "{:?} {:?} {:?} {:?}", opts.question, opts.file, opts.name, opts.task);
            found += usize::from(now["ok"] == json!(true));
        }
        assert!(found >= 12, "the questions that find something must answer: {found}");

        let empty = tempdir().unwrap();
        for opts in every_question(empty.path(), &skill) {
            let now = answered(&opts);
            assert_eq!(now, answered_from_the_whole_map(&opts), "{:?} {:?} {:?}", opts.question, opts.file, opts.name);
        }
    }

    /// Antes de cada resposta, o mapa se confere com o conteúdo de agora:
    /// um arquivo editado sem commit é relido, e a pergunta seguinte já
    /// responde a linha nova; sem nada mudado, o scan não roda.
    #[test]
    fn a_question_after_an_edit_without_commit_answers_the_new_line() {
        let scan = mustard_core::Scan::locate();
        assert!(
            scan.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q"]);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() -> u32 {\n    1\n}\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        let calls = std::cell::Cell::new(0);
        let mine = |root: &Path, out: &Path| {
            calls.set(calls.get() + 1);
            scan.scan(root, out)
        };
        scan.scan(root, &store::model_path(root)).expect("the first pass writes the map");

        let no_trace = |_: &Path, _: &Path, _: &str, _: usize| panic!("the users question never reads a history");
        let mut opts = ask(root, Question::Users);
        opts.name = Some("alpha".to_string());
        assert_eq!(map_at(&opts, &mine, &no_trace)["declarations"][0]["line"], json!(1));
        assert_eq!(calls.get(), 0, "nothing changed: the scan does not run");

        std::fs::write(root.join("src/lib.rs"), "// topo\n\npub fn alpha() -> u32 {\n    1\n}\n").unwrap();
        let report = map_at(&opts, &mine, &no_trace);
        assert_eq!(report["declarations"][0]["line"], json!(3), "{report}");
        assert_eq!(calls.get(), 1, "the edit is read once");
        assert_eq!(map_at(&opts, &mine, &no_trace)["declarations"][0]["line"], json!(3));
        assert_eq!(calls.get(), 1, "and not again while nothing else changes");
    }

    /// A lista gravada de `apps/rt/src/commands/pay/write.rs`, da base
    /// `main`, montada quando o commit mais novo do arquivo era o `c3`: a
    /// primeira `run` mudou no `c3` e, só na forma, no `c1`; a segunda não
    /// tem commit na base.
    fn write_rs_lineage() -> FileLineage {
        FileLineage {
            path: "apps/rt/src/commands/pay/write.rs".to_string(),
            base: "main".to_string(),
            last_commit: "c3".to_string(),
            mark: String::new(),
            moves: u32::try_from(project_map::MOVES_FOLLOWED).unwrap(),
            commits: vec![
                LineageCommit { id: "c3".to_string(), at: 259_200, title: "feat(pay): grava o pagamento (#4)".to_string(), pr: Some(4), ..LineageCommit::default() },
                LineageCommit { id: "c1".to_string(), at: 86_400, title: "formata".to_string(), pr: None, ..LineageCommit::default() },
            ],
            declarations: vec![DeclLineage {
                name: "run".to_string(),
                nth: 0,
                commits: vec![DeclChange { id: "c3".to_string(), form: false }, DeclChange { id: "c1".to_string(), form: true }],
                comments: Vec::new(),
            }],
            comments: 0,
        }
    }

    /// O mapa de todas as partes, com a base `main` e a lista gravada de um
    /// arquivo ainda valendo.
    fn map_with_a_base_and_a_lineage() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let mut map: Value = serde_json::from_str(EVERY_PART).unwrap();
        map["history"]["base"] = json!("main");
        store::write_text(dir.path(), &map.to_string()).unwrap();
        store::save_lineage_at(&store::model_path(dir.path()), &write_rs_lineage()).unwrap();
        dir
    }

    /// A lista de `write.rs` com doze commits na primeira `run`, do `k12`, o
    /// mais novo, ao `k01`, lida seguindo `moves` mudanças de arquivo.
    fn long_lineage(moves: usize) -> FileLineage {
        let ids: Vec<String> = (1..=12).rev().map(|n| format!("k{n:02}")).collect();
        FileLineage {
            moves: u32::try_from(moves).unwrap(),
            commits: ids
                .iter()
                .zip(0_i64..)
                .map(|(id, age)| LineageCommit { id: id.clone(), at: 1_000_000 - age * 1_000, title: format!("muda {id}"), pr: None, ..LineageCommit::default() })
                .collect(),
            declarations: vec![DeclLineage {
                name: "run".to_string(),
                nth: 0,
                commits: ids.iter().map(|id| DeclChange { id: id.clone(), form: false }).collect(),
                comments: Vec::new(),
            }],
            ..write_rs_lineage()
        }
    }

    /// O mapa com a base `main` e a lista longa de `write.rs`, montada com
    /// o número de mudanças do padrão; a pergunta da história da `run` dele,
    /// na sessão `session`; a passada falsa, que conta os números com que
    /// rodou e grava a lista longa com o número pedido.
    struct HistoryNumbers {
        dir: tempfile::TempDir,
        traced: std::cell::RefCell<Vec<usize>>,
    }

    impl HistoryNumbers {
        fn new() -> Self {
            let dir = map_with_a_base_and_a_lineage();
            store::save_lineage_at(&store::model_path(dir.path()), &long_lineage(project_map::MOVES_FOLLOWED)).unwrap();
            Self { dir, traced: std::cell::RefCell::new(Vec::new()) }
        }

        fn config(&self, text: &str) {
            std::fs::write(self.dir.path().join("mustard.json"), text).unwrap();
        }

        fn ask(&self, session: &str) -> Value {
            let opts = MapOpts {
                file: Some("apps/rt/src/commands/pay/write.rs".to_string()),
                name: Some("run".to_string()),
                session: Some(session.to_string()),
                ..ask(self.dir.path(), Question::History)
            };
            let trace = |_: &Path, out: &Path, _: &str, moves: usize| {
                self.traced.borrow_mut().push(moves);
                store::save_lineage_at(out, &long_lineage(moves))?;
                Ok(HistoryReport::default())
            };
            map_at(&opts, &|_, _| panic!("a map outside git is never read again"), &trace)
        }

        fn shown(report: &Value) -> usize {
            report["declarations"][0]["commits"].as_array().map_or(0, Vec::len)
        }
    }

    /// Sem as chaves no `mustard.json`, a resposta traz os commits do
    /// padrão, e a lista gravada seguindo as mudanças do padrão vale, sem
    /// passada nem aviso.
    #[test]
    fn the_history_without_the_keys_uses_the_default_numbers() {
        let case = HistoryNumbers::new();
        case.config(r#"{"git": {"flow": {"*": "main"}}}"#);
        let report = case.ask("sessao");
        assert_eq!(HistoryNumbers::shown(&report), project_map::DECL_COMMITS_SHOWN, "{report}");
        assert!(report.get("warnings").is_none(), "{report}");
        assert!(case.traced.borrow().is_empty(), "the list read with the default number still counts");
    }

    /// Com `historyCommits` em 3, a resposta traz 3 commits; com
    /// `historyMoves` em 2, a lista gravada com outro número se monta de
    /// novo, seguindo só 2 mudanças de arquivo, e a pergunta seguinte a lê.
    #[test]
    fn the_history_keys_set_the_commits_shown_and_the_moves_followed() {
        let case = HistoryNumbers::new();
        case.config(r#"{"map": {"historyCommits": 3}}"#);
        let report = case.ask("sessao");
        assert_eq!(HistoryNumbers::shown(&report), 3, "{report}");
        assert!(case.traced.borrow().is_empty(), "{report}");

        case.config(r#"{"map": {"historyMoves": 2}}"#);
        let report = case.ask("sessao");
        assert_eq!(*case.traced.borrow(), vec![2], "{report}");
        assert_eq!(HistoryNumbers::shown(&report), project_map::DECL_COMMITS_SHOWN, "{report}");
        case.ask("sessao");
        assert_eq!(*case.traced.borrow(), vec![2], "the list read with 2 counts for the next question");
    }

    /// O valor inválido — zero, negativo ou texto — cai no padrão e sai com
    /// um aviso que diz a chave e o padrão, uma vez só na sessão; outra
    /// sessão recebe o aviso de novo.
    #[test]
    fn an_invalid_history_key_falls_back_to_the_default_with_one_warning() {
        for bad in ["0", "-2", "\"dez\""] {
            let case = HistoryNumbers::new();
            case.config(&format!(r#"{{"map": {{"historyCommits": {bad}, "historyMoves": {bad}, "pullRequestCalls": {bad}}}}}"#));
            let report = case.ask("sessao-1");
            assert_eq!(HistoryNumbers::shown(&report), project_map::DECL_COMMITS_SHOWN, "{bad}: {report}");
            assert!(case.traced.borrow().is_empty(), "{bad}: the default moves still count: {report}");
            let warnings: Vec<&str> =
                report["warnings"].as_array().map(|all| all.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            assert_eq!(warnings.len(), 3, "{bad}: {report}");
            assert!(warnings[0].contains("map.historyMoves") && warnings[0].contains(&project_map::MOVES_FOLLOWED.to_string()));
            assert!(warnings[1].contains("map.historyCommits") && warnings[1].contains(&project_map::DECL_COMMITS_SHOWN.to_string()));
            let calls = crate::shared::pr_history::CALLS_PER_PASS.to_string();
            assert!(warnings[2].contains("map.pullRequestCalls") && warnings[2].contains(&calls), "{bad}: {report}");

            let again = case.ask("sessao-1");
            assert!(again.get("warnings").is_none(), "{bad}: one warning per session: {again}");
            assert_eq!(HistoryNumbers::shown(&again), project_map::DECL_COMMITS_SHOWN, "{bad}: {again}");
            assert!(case.ask("sessao-2").get("warnings").is_some(), "{bad}: another session is warned again");
        }
    }

    /// Na busca, o valor inválido de `map.historyMoves` — zero, negativo ou
    /// texto — cai no padrão e sai com o mesmo aviso da pergunta da história,
    /// uma vez só na sessão, seja qual for a pergunta que o leu primeiro:
    /// depois da busca, a história na mesma sessão não avisa de novo, e segue
    /// o padrão. Outra sessão recebe o aviso de novo; o valor certo, nenhum.
    #[test]
    fn an_invalid_history_moves_warns_in_the_search_once_per_session_with_the_history_question() {
        for bad in ["0", "-2", "\"dez\""] {
            let case = HistoryNumbers::new();
            case.config(&format!(r#"{{"map": {{"historyMoves": {bad}}}, "search": {{"filter": "none"}}}}"#));
            let search = |session: &str| {
                searched(&search_opts(case.dir.path(), "pay", None, Some(session)), &|_, _| {
                    panic!("the filter is off")
                })
            };
            let report = search("sessao-1");
            assert_eq!(report["ok"], json!(true), "{bad}: {report}");
            let warnings: Vec<&str> =
                report["warnings"].as_array().map(|all| all.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            assert_eq!(warnings.len(), 1, "{bad}: {report}");
            assert!(warnings[0].contains("map.historyMoves"), "{bad}: {report}");
            assert!(warnings[0].contains(&project_map::MOVES_FOLLOWED.to_string()), "{bad}: {report}");

            let again = search("sessao-1");
            assert!(again.get("warnings").is_none(), "{bad}: one warning per session: {again}");
            let history = case.ask("sessao-1");
            assert!(history.get("warnings").is_none(), "{bad}: the search already warned the session: {history}");
            assert!(case.traced.borrow().is_empty(), "{bad}: the default moves still count: {history}");
            assert!(search("sessao-2").get("warnings").is_some(), "{bad}: another session is warned again");

            case.config(r#"{"map": {"historyMoves": 2}, "search": {"filter": "none"}}"#);
            let valid = search("sessao-3");
            assert_eq!(valid["ok"], json!(true), "{bad}: {valid}");
            assert!(valid.get("warnings").is_none(), "{bad}: {valid}");
        }
    }

    /// O mapa com a base `main`, a lista de `write.rs` com a `run` mudada
    /// por dois commits do pull request 4 e um sem número, o comentário de
    /// revisão preso a ela, de 300 letras, e o texto do pull request 4, com a
    /// descrição de dois parágrafos, o primeiro de 700 letras.
    fn map_with_a_pull_request() -> tempfile::TempDir {
        let dir = map_with_a_base_and_a_lineage();
        let model = store::model_path(dir.path());
        let path = "apps/rt/src/commands/pay/write.rs";
        let commit = |id: &str, at: i64, title: &str, pr: Option<u32>| LineageCommit { id: id.to_string(), at, title: title.to_string(), pr, ..LineageCommit::default() };
        let lineage = FileLineage {
            commits: vec![
                commit("c3", 259_200, "feat(pay): grava o pagamento (#4)", Some(4)),
                commit("c2", 172_800, "fix(pay): arredonda (#4)", Some(4)),
                commit("c1", 86_400, "formata", None),
            ],
            declarations: vec![DeclLineage {
                name: "run".to_string(),
                nth: 0,
                commits: ["c3", "c2", "c1"].iter().map(|id| DeclChange { id: (*id).to_string(), form: false }).collect(),
                comments: vec![DeclComment { pr: 4, commit: "c2".to_string(), body: "a".repeat(300) }],
            }],
            comments: 1,
            ..write_rs_lineage()
        };
        store::save_lineage_at(&model, &lineage).unwrap();
        let text = PullText {
            number: 4,
            title: "Grava o pagamento".to_string(),
            body: format!("{}\r\n\r\nSegundo parágrafo, que não aparece.", "p".repeat(700)),
            etag: String::new(),
            through: "c3".to_string(),
        };
        let comment = PullComment { number: 4, commit: "c2".to_string(), path: path.to_string(), line: 2, body: "a".repeat(300) };
        store::save_pull_at(&model, &text, &[comment]).unwrap();
        dir
    }

    /// A história mostra o título de cada pull request uma vez, e o
    /// comentário de revisão preso às linhas da função com até 200 letras;
    /// `--pr` mostra o primeiro parágrafo da descrição, até 600 letras, com o
    /// nome ou sozinho, e diz quando o mapa ainda não tem o texto.
    #[test]
    fn the_history_shows_each_pull_request_once_the_comments_and_the_description_asked() {
        let dir = map_with_a_pull_request();
        let with = |name: Option<&str>, pr: Option<u32>| MapOpts {
            file: Some("apps/rt/src/commands/pay/write.rs".to_string()),
            name: name.map(str::to_string),
            pr,
            ..ask(dir.path(), Question::History)
        };
        let report = answered(&with(Some("run"), None));
        let run = &report["declarations"][0];
        assert_eq!(run["pulls"], json!(["#4 Grava o pagamento"]), "{report}");
        let comments = run["comments"].as_array().unwrap();
        assert_eq!(comments.len(), 1, "{report}");
        let body = comments[0].as_str().unwrap().strip_prefix("#4 ").unwrap();
        assert_eq!(body.chars().count(), 200, "{report}");
        assert!(body.ends_with('…'), "{report}");
        assert!(report.get("pull").is_none(), "{report}");

        let both = answered(&with(Some("run"), Some(4)));
        assert_eq!(both["declarations"], report["declarations"], "{both}");
        let pull = &both["pull"];
        assert_eq!((pull["number"].clone(), pull["title"].clone()), (json!(4), json!("Grava o pagamento")), "{both}");
        let description = pull["description"].as_str().unwrap();
        assert_eq!(description.chars().count(), 600, "{both}");
        assert!(description.starts_with("ppp") && !description.contains("Segundo"), "{both}");

        let alone = answered(&MapOpts { file: None, ..with(None, Some(4)) });
        assert_eq!(alone, json!({ "ok": true, "question": "history", "pull": both["pull"] }), "{alone}");
        let missing = answered(&MapOpts { file: None, ..with(None, Some(5)) });
        assert!(missing["pull"]["note"].as_str().unwrap().contains("#5"), "{missing}");
        assert!(missing["pull"].get("description").is_none(), "{missing}");

        for opts in [with(Some("run"), Some(4)), with(None, Some(5))] {
            assert_eq!(answered(&opts), answered_from_the_whole_map(&opts), "{:?}", opts.pr);
        }
    }

    /// Um comentário de revisão lido depois que a lista do arquivo se
    /// montou vence a lista: a pergunta seguinte a monta de novo, e o
    /// comentário aparece; a pergunta depois dela lê a lista nova sem
    /// passada.
    #[test]
    fn a_comment_read_after_the_list_was_built_shows_in_the_next_question() {
        let dir = map_with_a_base_and_a_lineage();
        let model = store::model_path(dir.path());
        let traced = std::cell::Cell::new(0);
        let trace = |_: &Path, out: &Path, file: &str, _: usize| {
            traced.set(traced.get() + 1);
            let comments = store::pull_comments_at(out, file).unwrap();
            let mut lineage = write_rs_lineage();
            lineage.comments = u32::try_from(comments.len()).unwrap();
            lineage.declarations[0].comments =
                comments.iter().map(|c| DeclComment { pr: c.number, commit: c.commit.clone(), body: c.body.clone() }).collect();
            store::save_lineage_at(out, &lineage)?;
            Ok(HistoryReport::default())
        };
        let opts = MapOpts {
            file: Some("apps/rt/src/commands/pay/write.rs".to_string()),
            name: Some("run".to_string()),
            ..ask(dir.path(), Question::History)
        };
        let ask_now = || map_at(&opts, &|_, _| panic!("a map outside git is never read again"), &trace);
        let before = ask_now();
        assert_eq!(traced.get(), 0, "{before}");
        assert!(before["declarations"][0].get("comments").is_none(), "{before}");

        let text = PullText { number: 4, title: "Grava o pagamento".to_string(), ..PullText::default() };
        let comment = PullComment { number: 4, commit: "c3".to_string(), path: "apps/rt/src/commands/pay/write.rs".to_string(), line: 2, body: "cuidado com o arredondamento".to_string() };
        store::save_pull_at(&model, &text, &[comment]).unwrap();
        let after = ask_now();
        assert_eq!(traced.get(), 1, "{after}");
        assert_eq!(after["declarations"][0]["comments"], json!(["#4 cuidado com o arredondamento"]), "{after}");
        ask_now();
        assert_eq!(traced.get(), 1, "the list built with the comment counts for the next question");
    }

    /// A pergunta da história lê pela porta só as declarações do nome, a
    /// história guardada e a lista do arquivo, e responde o mesmo que com o
    /// mapa inteiro; com a lista valendo, a passada não roda.
    #[test]
    fn the_history_question_reading_its_tables_answers_as_the_whole_map_did() {
        let dir = map_with_a_base_and_a_lineage();
        let with = |file: Option<&str>, name: Option<&str>| MapOpts {
            file: file.map(str::to_string),
            name: name.map(str::to_string),
            ..ask(dir.path(), Question::History)
        };
        for opts in [
            with(Some("apps/rt/src/commands/pay/write.rs"), Some("run")),
            with(Some("./apps/rt/src/commands/pay/write.rs"), Some(" run ")),
            with(None, Some("run")),
            with(None, Some("sumiu")),
            with(None, None),
        ] {
            assert_eq!(answered(&opts), answered_from_the_whole_map(&opts), "{:?} {:?}", opts.file, opts.name);
        }
    }

    /// A resposta diz a função com o arquivo e a linha, quantas mudanças a
    /// base tem dela fora as só de forma, cada commit com a data, o começo do
    /// hash, o título sem o prefixo do tipo nem o número, e o número no fim,
    /// o commit só de forma marcado, e o `git show` do mais novo. A de mesmo
    /// nome que a base ainda não tem diz isso.
    #[test]
    fn the_history_answer_lists_the_commits_with_the_clean_title_the_number_and_the_mark() {
        let dir = map_with_a_base_and_a_lineage();
        let mut opts = ask(dir.path(), Question::History);
        opts.file = Some("apps/rt/src/commands/pay/write.rs".to_string());
        opts.name = Some("run".to_string());
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        let first = &report["declarations"][0];
        assert_eq!((first["line"].clone(), first["changes"].clone()), (json!(2), json!(1)), "{report}");
        let lines: Vec<&str> = first["commits"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect();
        assert_eq!(lines.len(), 2, "{report}");
        assert!(lines[0].ends_with(" c3 grava o pagamento #4"), "{report}");
        let form = mustard_core::translate("map.history.form", Locale::default());
        assert!(lines[1].ends_with(&format!(" c1 formata {form}")), "{report}");
        assert!(report["next"].as_str().unwrap().contains("git show c3"), "{report}");
        let second = &report["declarations"][1];
        assert_eq!(second["line"], json!(6), "{report}");
        assert!(second["note"].as_str().unwrap().contains("main"), "{report}");
    }

    /// Sem `--name`, a recusa diz a opção que falta; sem base declarada, a
    /// resposta diz que não há história e como declarar; o nome em mais de
    /// um arquivo, sem `--file`, lista os lugares e pede o arquivo. Em
    /// nenhum dos três a passada roda.
    #[test]
    fn the_history_question_without_name_without_base_or_with_the_name_in_two_files_does_not_trace() {
        let plain = tempdir().unwrap();
        store::write_text(plain.path(), EVERY_PART).unwrap();
        let mut opts = ask(plain.path(), Question::History);
        let report = answered(&opts);
        assert_eq!(report["reason"], json!("missing-argument"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("--name"), "{report}");

        opts.name = Some("Payment".to_string());
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        let no_base = mustard_core::translate("map.history.no_base", Locale::default());
        assert_eq!(report["note"], json!(no_base), "{report}");
        assert!(no_base.contains("mustard.json"));

        let dir = map_with_a_base_and_a_lineage();
        let mut opts = ask(dir.path(), Question::History);
        opts.name = Some("run".to_string());
        let report = answered(&opts);
        assert_eq!(
            report["places"],
            json!([
                "apps/rt/src/commands/pay/gen.rs:1",
                "apps/rt/src/commands/pay/index.rs:5",
                "apps/rt/src/commands/pay/read.rs:1",
                "apps/rt/src/commands/pay/write.rs:2",
                "apps/rt/src/commands/pay/write.rs:6",
                "web/src/pay.ts:3"
            ]),
            "{report}"
        );
        assert!(report["note"].as_str().unwrap().contains("--file"), "{report}");
    }

    /// Num repositório de verdade, com o scan compilado junto: a primeira
    /// pergunta sobre um arquivo monta a lista dele, e a segunda a lê sem a
    /// passada. Um commit novo da base que toca só um arquivo, somado pela
    /// montagem, vence só a lista dele.
    #[test]
    fn the_history_is_traced_once_per_file_and_again_only_for_the_file_a_new_commit_touched() {
        let scan = mustard_core::Scan::locate();
        assert!(
            scan.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        let commit = |rel: &str, body: &str, title: &str| {
            std::fs::create_dir_all(root.join(rel).parent().unwrap()).unwrap();
            std::fs::write(root.join(rel), body).unwrap();
            git(&["add", "-A"]);
            git(&["commit", "-q", "-m", title]);
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(root.join(".git/info/exclude"), "mustard.json
.claude/
").unwrap();
        std::fs::write(root.join("mustard.json"), r#"{"git": {"flow": {"*": "main"}}}"#).unwrap();
        let gravar = |n: u32| format!("pub fn gravar() -> u32 {{
    {n}
}}
");
        commit("src/a.rs", &gravar(1), "feat(a): cria o gravar");
        commit("src/b.rs", "pub fn ler() -> u32 {
    1
}
", "cria o ler");
        commit("src/c.rs", "pub fn gravar() {}
", "outro gravar");
        commit("src/a.rs", &gravar(2), "fix(a): corrige o gravar (#5)");
        commit("src/a.rs", &gravar(3), "muda o gravar de novo");
        scan.scan(root, &store::model_path(root)).expect("the first pass writes the map");
        let (mines, traces) = (std::cell::Cell::new(0), std::cell::Cell::new(0));
        let mine = |root: &Path, out: &Path| {
            mines.set(mines.get() + 1);
            scan.scan(root, out)
        };
        let trace = |root: &Path, out: &Path, file: &str, moves: usize| {
            traces.set(traces.get() + 1);
            scan.history(root, out, file, moves)
        };
        let question = |file: Option<&str>, name: &str| MapOpts {
            file: file.map(str::to_string),
            name: Some(name.to_string()),
            ..ask(root, Question::History)
        };
        let lines = |report: &Value| -> Vec<String> {
            report["declarations"][0]["commits"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|line| line.as_str().unwrap_or_default().to_string())
                .collect()
        };

        let report = map_at(&question(Some("src/a.rs"), "gravar"), &mine, &trace);
        let got = lines(&report);
        assert_eq!(got.len(), 3, "{report}");
        assert!(got[0].ends_with(" muda o gravar de novo"), "{report}");
        assert!(got[1].ends_with(" corrige o gravar #5"), "{report}");
        assert!(got[2].ends_with(" cria o gravar"), "{report}");
        assert_eq!(report["declarations"][0]["changes"], json!(3), "{report}");
        assert_eq!(traces.get(), 1);

        assert_eq!(lines(&map_at(&question(Some("src/a.rs"), "gravar"), &mine, &trace)), got);
        assert_eq!(traces.get(), 1, "the second question reads the stored list");

        let both = map_at(&question(None, "gravar"), &mine, &trace);
        assert_eq!(both["places"], json!(["src/a.rs:1", "src/c.rs:1"]), "{both}");
        assert_eq!(traces.get(), 1, "the name in two files lists the places without tracing");

        map_at(&question(None, "ler"), &mine, &trace);
        assert_eq!(traces.get(), 2);
        assert_eq!(mines.get(), 0, "nothing changed in the project");

        commit("src/a.rs", &gravar(4), "muda o gravar pela quarta vez");
        let report = map_at(&question(None, "ler"), &mine, &trace);
        assert_eq!(lines(&report).len(), 1, "{report}");
        assert_eq!(mines.get(), 1, "the new commit is summed by the pass that reads what changed");
        assert_eq!(traces.get(), 2, "the commit did not touch the file of `ler`");

        let report = map_at(&question(Some("src/a.rs"), "gravar"), &mine, &trace);
        assert_eq!(traces.get(), 3, "the commit touched the file of `gravar`");
        assert!(lines(&report)[0].ends_with(" muda o gravar pela quarta vez"), "{report}");
    }

    #[test]
    fn a_skill_citing_a_path_that_does_not_exist_is_refused() {
        let dir = project_with_map();
        let skill_dir = dir.path().join("apps").join("rt").join(".claude").join("skills").join("add-pay");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_path = skill_dir.join("SKILL.md");
        std::fs::write(&skill_path, "Veja `pay/write.rs` e `apps/rt/src/commands/pay/sumiu.rs`.\n").unwrap();
        let mut opts = ask(dir.path(), Question::Skill);
        opts.path = Some(skill_path.clone());
        let report = answered(&opts);
        assert_eq!(report["reason"], json!("skill-missing-path"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("apps/rt/src/commands/pay/sumiu.rs"));

        std::fs::write(&skill_path, "Veja `pay/write.rs` e `commands/pay/index.rs`.\n").unwrap();
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
    }

    /// O mapa com a base, a lista de `write.rs` e a spec `obra`: a decisão
    /// do estorno, a tarefa da onda 1 que a cobre e muda `write.rs`, e o
    /// commit da onda 1, o `c3`, que a lista diz ter mudado a `run`.
    fn map_with_a_spec() -> tempfile::TempDir {
        let dir = map_with_a_base_and_a_lineage();
        let spec = dir.path().join(".claude/spec/obra");
        std::fs::create_dir_all(&spec).unwrap();
        let lines = [
            json!({"id": 1, "type": "decision", "title": "Estorno volta ao cartão",
                   "text": "O estorno volta ao cartão em dois dias.\nNunca em dinheiro.", "keys": ["estorno"],
                   "why": "w", "origin": 1}),
            json!({"id": 2, "type": "task", "wave": 1, "title": "Grava a volta", "text": "Grava a volta.",
                   "files": [{"path": "apps/rt/src/commands/pay/write.rs"}], "covers": [1], "depends_on": []}),
            json!({"id": 3, "type": "commit", "sha": "c3", "title": "t", "waves": [1],
                   "files": ["apps/rt/src/commands/pay/write.rs"], "repo": "r"}),
        ];
        let text: String = lines.iter().map(|line| line.to_string() + "\n").collect();
        std::fs::write(spec.join("spec.ndjson"), text).unwrap();
        dir
    }

    /// A busca por uma palavra de uma decisão devolve a decisão, com o
    /// código, o título, a linha da parte do usuário que casou e a função
    /// que o commit da onda dela mudou, pelo que o mapa guarda: com o
    /// arquivo da spec mudado por baixo, no mesmo tamanho e na mesma hora,
    /// a resposta seguinte continua a do mapa, porque a pergunta não abre o
    /// arquivo.
    #[test]
    fn a_search_finds_the_decision_and_its_function_without_opening_the_spec_file() {
        let dir = map_with_a_spec();
        let mut opts = ask(dir.path(), Question::Search);
        opts.query = Some("estorno".to_string());
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        let first = &report["specs"][0];
        assert_eq!(
            first,
            &json!({"spec": "obra", "code": "MSTD-DEC-0001", "title": "Estorno volta ao cartão",
                    "line": "O estorno volta ao cartão em dois dias.",
                    "links": ["apps/rt/src/commands/pay/write.rs:run"]}),
            "{report}"
        );

        let path = dir.path().join(".claude/spec/obra/spec.ndjson");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("Estorno volta ao cartão", "Estorno volta ao CARTÃO")).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
        let again = answered(&opts);
        assert_eq!(again["specs"][0]["title"], json!("Estorno volta ao cartão"), "the answer came from the map: {again}");
    }

    /// Grava em `root` a spec `obra`: a decisão do estorno, a tarefa da onda
    /// 1 que a cobre e o commit `sha` da onda 1, que mudou `changed`.
    fn spec_with_a_wave_commit(root: &Path, sha: &str, changed: &[&str]) {
        let spec = root.join(".claude/spec/obra");
        std::fs::create_dir_all(&spec).unwrap();
        let lines = [
            json!({"id": 1, "type": "decision", "title": "Estorno volta ao cartão",
                   "text": "O estorno volta ao cartão em dois dias.", "keys": ["estorno"], "why": "w", "origin": 1}),
            json!({"id": 2, "type": "task", "wave": 1, "title": "Grava a volta", "text": "Grava a volta.",
                   "files": [{"path": changed[0]}], "covers": [1], "depends_on": []}),
            json!({"id": 3, "type": "commit", "sha": sha, "title": "t", "waves": [1], "files": changed, "repo": "r"}),
        ];
        let text: String = lines.iter().map(|line| line.to_string() + "\n").collect();
        std::fs::write(spec.join("spec.ndjson"), text).unwrap();
    }

    /// Os lugares que a busca por `estorno` liga à decisão, pela passada
    /// `trace`.
    fn refund_links(root: &Path, trace: &Trace<'_>) -> Value {
        let mut opts = ask(root, Question::Search);
        opts.query = Some("estorno".to_string());
        let report = map_at(&opts, &|_, _| panic!("a map outside git is never read again"), trace);
        assert_eq!(report["ok"], json!(true), "{report}");
        let found = report["specs"].as_array().unwrap().iter().find(|item| item["code"] == json!("MSTD-DEC-0001"));
        found.unwrap_or_else(|| panic!("the decision: {report}"))["links"].clone()
    }

    /// Com a base, a busca monta só a história do arquivo que o commit da
    /// onda mudou, que o mapa tem e que ainda não a tem: nem a do arquivo
    /// que já a tem, nem a do que o mapa não tem. A passada que falha deixa
    /// o arquivo inteiro, e a busca responde. Sem a base, a busca não monta
    /// história nenhuma, e cada arquivo entra inteiro.
    #[test]
    fn a_search_traces_only_the_mapped_file_without_a_history_and_nothing_without_a_base() {
        let dir = map_with_a_base_and_a_lineage();
        let root = dir.path();
        let changed = ["apps/rt/src/commands/pay/write.rs", "apps/rt/src/commands/pay/index.rs", "src/sumiu.rs"];
        spec_with_a_wave_commit(root, "c3", &changed);
        let traced = std::cell::RefCell::new(Vec::new());
        let failing = |_: &Path, _: &Path, file: &str, _: usize| -> mustard_core::platform::error::Result<HistoryReport> {
            traced.borrow_mut().push(file.to_string());
            Err(std::io::Error::other("git ilegível").into())
        };
        let links = refund_links(root, &failing);
        assert_eq!(*traced.borrow(), ["apps/rt/src/commands/pay/index.rs"]);
        assert_eq!(
            links,
            json!(["apps/rt/src/commands/pay/write.rs:run", "apps/rt/src/commands/pay/index.rs", "src/sumiu.rs"])
        );

        let dir = tempdir().unwrap();
        store::write_text(dir.path(), EVERY_PART).unwrap();
        spec_with_a_wave_commit(dir.path(), "c3", &changed);
        let links = refund_links(dir.path(), &|_, _, _, _| panic!("a map without a base never reads a history"));
        assert_eq!(links, json!(changed));
    }

    /// Num repositório de verdade, com o scan compilado junto: o commit da
    /// onda mudou um arquivo que ainda não tem a história por função, e a
    /// primeira busca já liga a decisão à função que ele mudou, e não ao
    /// arquivo inteiro. A busca monta a história desse arquivo uma vez só:
    /// a segunda a lê gravada, sem passada.
    #[test]
    fn a_search_traces_the_file_its_wave_changed_once_and_links_to_the_function() {
        let scan = mustard_core::Scan::locate();
        assert!(
            scan.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| -> String {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let pay = |total: u32| format!("pub fn pay() -> u32 {{\n    {total}\n}}\n\npub fn refund() -> u32 {{\n    2\n}}\n");
        let commit = |body: &str, title: &str| {
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::write(root.join("src/pay.rs"), body).unwrap();
            git(&["add", "-A"]);
            git(&["commit", "-q", "-m", title]);
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(root.join(".git/info/exclude"), "mustard.json\n.claude/\n").unwrap();
        std::fs::write(root.join("mustard.json"), r#"{"git": {"flow": {"*": "main"}}}"#).unwrap();
        commit(&pay(1), "cria o pagamento");
        commit(&pay(3), "arredonda o pagamento");
        let sha = git(&["rev-parse", "HEAD"]);
        scan.scan(root, &store::model_path(root)).expect("the first pass writes the map");
        spec_with_a_wave_commit(root, &sha, &["src/pay.rs"]);
        let traces = std::cell::Cell::new(0);
        let trace = |root: &Path, out: &Path, file: &str, moves: usize| {
            traces.set(traces.get() + 1);
            scan.history(root, out, file, moves)
        };
        let mine = |root: &Path, out: &Path| scan.scan(root, out);
        let links = || {
            let mut opts = ask(root, Question::Search);
            opts.query = Some("estorno".to_string());
            let report = map_at(&opts, &mine, &trace);
            assert_eq!(report["ok"], json!(true), "{report}");
            report["specs"][0]["links"].clone()
        };

        assert_eq!(links(), json!(["src/pay.rs:pay"]));
        assert_eq!(traces.get(), 1);
        assert_eq!(links(), json!(["src/pay.rs:pay"]));
        assert_eq!(traces.get(), 1, "the second search reads the stored history");
    }

    /// Num repositório de verdade, com o scan compilado junto: a obra ainda
    /// fora da base, a busca monta a história do arquivo que a onda mudou, e
    /// o item liga ao arquivo inteiro, porque a base não tem o commit dela.
    /// Depois o squash da obra, com o número do pull request da spec, entra
    /// na base e muda o arquivo: a história guardada venceu, e a busca
    /// seguinte a monta de novo e liga o item à função que o squash mudou. A
    /// terceira busca a lê gravada, sem passada.
    #[test]
    fn a_search_rebuilds_the_history_the_squash_of_its_spec_expired_and_links_to_the_function() {
        let scan = mustard_core::Scan::locate();
        assert!(
            scan.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| -> String {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let pay = |total: u32| format!("pub fn pay() -> u32 {{\n    {total}\n}}\n\npub fn refund() -> u32 {{\n    2\n}}\n");
        let write_pay = |total: u32| {
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::write(root.join("src/pay.rs"), pay(total)).unwrap();
            git(&["add", "-A"]);
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(root.join(".git/info/exclude"), "mustard.json\n.claude/\n").unwrap();
        std::fs::write(root.join("mustard.json"), r#"{"git": {"flow": {"*": "main"}}}"#).unwrap();
        write_pay(1);
        git(&["commit", "-q", "-m", "cria o pagamento"]);
        git(&["checkout", "-q", "-b", "obra"]);
        write_pay(3);
        git(&["commit", "-q", "-m", "arredonda o pagamento"]);
        let wave = git(&["rev-parse", "HEAD"]);
        git(&["checkout", "-q", "main"]);
        scan.scan(root, &store::model_path(root)).expect("the first pass writes the map");
        spec_with_a_wave_commit(root, &wave, &["src/pay.rs"]);
        let log = root.join(".claude/spec/obra/spec.ndjson");
        let opened = json!({"id": 4, "type": "state", "phase": "pr_open", "pr": {"number": 7, "url": "u"}});
        std::fs::write(&log, std::fs::read_to_string(&log).unwrap() + &opened.to_string() + "\n").unwrap();
        let traces = std::cell::Cell::new(0);
        let trace = |root: &Path, out: &Path, file: &str, moves: usize| {
            traces.set(traces.get() + 1);
            scan.history(root, out, file, moves)
        };
        let mine = |root: &Path, out: &Path| scan.scan(root, out);
        let links = || {
            let mut opts = ask(root, Question::Search);
            opts.query = Some("estorno".to_string());
            let report = map_at(&opts, &mine, &trace);
            assert_eq!(report["ok"], json!(true), "{report}");
            report["specs"][0]["links"].clone()
        };

        assert_eq!(links(), json!(["src/pay.rs"]), "the base does not have the wave yet");
        assert_eq!(traces.get(), 1);
        git(&["merge", "-q", "--squash", "obra"]);
        git(&["commit", "-q", "-m", "feat(obra): arredonda o pagamento (#7)"]);
        assert_eq!(links(), json!(["src/pay.rs:pay"]), "the squash expired the stored history");
        assert_eq!(traces.get(), 2);
        assert_eq!(links(), json!(["src/pay.rs:pay"]));
        assert_eq!(traces.get(), 2, "the rebuilt history is read from the map");
    }

    /// A busca e a pergunta da história conferem a história guardada de um
    /// arquivo pela mesma regra: valendo, nenhuma das duas a monta de novo;
    /// vencida pelo commit mais novo do arquivo na base, pela base, pela
    /// marca do scan ou pelo número de mudanças de arquivo seguidas, as duas
    /// a montam.
    #[test]
    fn the_search_and_the_history_question_rebuild_the_same_stored_histories() {
        let write = "apps/rt/src/commands/pay/write.rs";
        let other_moves = u32::try_from(project_map::MOVES_FOLLOWED + 1).unwrap();
        let cases = [
            ("fresh", write_rs_lineage()),
            ("older commit", FileLineage { last_commit: "c1".to_string(), ..write_rs_lineage() }),
            ("other base", FileLineage { base: "develop".to_string(), ..write_rs_lineage() }),
            ("other scan", FileLineage { mark: "outra".to_string(), ..write_rs_lineage() }),
            ("other moves", FileLineage { moves: other_moves, ..write_rs_lineage() }),
        ];
        for (case, lineage) in cases {
            let dir = map_with_a_base_and_a_lineage();
            let root = dir.path();
            store::save_lineage_at(&store::model_path(root), &lineage).unwrap();
            spec_with_a_wave_commit(root, "c3", &[write]);
            let traced = std::cell::RefCell::new(Vec::new());
            let trace = |_: &Path, _: &Path, file: &str, _: usize| -> mustard_core::platform::error::Result<HistoryReport> {
                traced.borrow_mut().push(file.to_string());
                Err(std::io::Error::other("git ilegível").into())
            };
            refund_links(root, &trace);
            let by_search = traced.take();
            let opts = MapOpts { file: Some(write.to_string()), name: Some("run".to_string()), ..ask(root, Question::History) };
            map_at(&opts, &|_, _| panic!("a map outside git is never read again"), &trace);
            let by_history = traced.take();
            assert_eq!(by_search, by_history, "{case}: the search and the history question agree");
            assert_eq!(by_search.is_empty(), case == "fresh", "{case}: {by_search:?}");
        }
    }

    /// Na história, o commit da onda ganha o código e a primeira frase do
    /// item combinado que ela cumpriu; o commit sem onda fica como era.
    #[test]
    fn the_history_line_of_a_wave_commit_brings_the_agreed_item() {
        let dir = map_with_a_spec();
        let mut opts = ask(dir.path(), Question::History);
        opts.file = Some("apps/rt/src/commands/pay/write.rs".to_string());
        opts.name = Some("run".to_string());
        let report = answered(&opts);
        let lines: Vec<&str> =
            report["declarations"][0]["commits"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect();
        let note = mustard_core::translate("map.history.spec", Locale::default())
            .replace("{spec}", "obra")
            .replace("{code}", "MSTD-DEC-0001")
            .replace("{sentence}", "O estorno volta ao cartão em dois dias.");
        assert!(lines[0].ends_with(&format!(" c3 grava o pagamento #4 — {note}")), "{report}");
        let form = mustard_core::translate("map.history.form", Locale::default());
        assert!(lines[1].ends_with(&format!(" c1 formata {form}")), "{report}");
        assert_eq!(report, answered_from_the_whole_map(&opts), "the whole map says the same");
    }

    /// Um mapa para a busca com filtro: três funções do pedido, uma do
    /// cliente, e um comentário no corpo, que nunca vai na resposta.
    const FILTER_MAP: &str = r#"{"modules": [
      {"path": "src/pedido.rs", "loc": 40, "declarations": [
        {"kind": "function", "name": "gravar_pedido", "line": 3, "end_line": 9,
         "signature": "pub fn gravar_pedido(pedido: &Pedido)",
         "doc": "Grava o pedido no banco. Depois avisa o cliente.", "body_comment": "trava a linha do pedido"},
        {"kind": "function", "name": "cancelar_pedido", "line": 11, "end_line": 20,
         "signature": "pub fn cancelar_pedido(id: u64)", "doc": "Cancela o pedido."},
        {"kind": "function", "name": "listar_pedidos", "line": 22, "end_line": 30,
         "signature": "pub fn listar_pedidos()", "doc": "Lista os pedidos do dia."}]},
      {"path": "src/cliente.rs", "loc": 20, "declarations": [
        {"kind": "function", "name": "avisar_cliente", "line": 1, "end_line": 5,
         "signature": "pub fn avisar_cliente()", "doc": "Avisa o cliente do pedido."}]}
    ]}"#;

    /// Um projeto com o mapa `map`, o texto em português e a seção `search`
    /// da configuração.
    fn search_project(map: &str, search: &Value) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        store::write_text(dir.path(), map).unwrap();
        let config = json!({"language": {"text": "pt-BR", "code": "pt-BR"}, "search": search});
        std::fs::write(dir.path().join("mustard.json"), config.to_string()).unwrap();
        dir
    }

    /// Um mapa com `count` funções que casam com "pedido", em arquivos de
    /// dez funções.
    fn many_orders(count: usize) -> String {
        let modules: Vec<Value> = (0..count.div_ceil(10))
            .map(|file| {
                let declarations: Vec<Value> = (0..10)
                    .map(|at| file * 10 + at)
                    .filter(|&n| n < count)
                    .map(|n| json!({"kind": "function", "name": format!("pedido_{n}"), "line": n + 1, "end_line": n + 1}))
                    .collect();
                json!({"path": format!("src/pedidos_{file}.rs"), "loc": 20, "declarations": declarations})
            })
            .collect();
        json!({ "modules": modules }).to_string()
    }

    fn search_opts(root: &Path, query: &str, intent: Option<&str>, session: Option<&str>) -> MapOpts {
        MapOpts {
            query: Some(query.to_string()),
            intent: intent.map(str::to_string),
            session: session.map(str::to_string),
            ..ask(root, Question::Search)
        }
    }

    /// A resposta da busca com a montagem `assemble` no lugar da chave do
    /// projeto.
    fn searched(opts: &MapOpts, assemble: &Assemble<'_>) -> Value {
        super::map_at(
            opts,
            &|_, _| panic!("a map outside git is never read again"),
            &|_, _, _, _| panic!("a map without a base never reads a history"),
            assemble,
        )
    }

    /// A busca do banco, a de antes do filtro, para `query`.
    fn bank_answer(root: &Path, query: &str) -> Value {
        let project = crate::commands::spec_events::project(root);
        bank_search(&project.root, query, &project.languages).unwrap()
    }

    /// A resposta sem o campo `field`.
    fn without(report: &Value, field: &str) -> Value {
        let mut report = report.clone();
        report.as_object_mut().unwrap().remove(field);
        report
    }

    /// Um filtro de mentira: guarda cada pedido e dá as notas de `notes` aos
    /// candidatos, na ordem do banco, com o corte de verdade; ou falha com
    /// `error`.
    #[derive(Clone)]
    struct FakeFilter {
        asked: std::rc::Rc<std::cell::RefCell<Vec<FilterRequest>>>,
        notes: Vec<f64>,
        error: Option<FilterError>,
    }

    impl MapFilter for FakeFilter {
        fn filter(&self, request: &FilterRequest) -> Result<Filtered, FilterError> {
            self.asked.borrow_mut().push(request.clone());
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            let scores: Vec<mustard_core::domain::map_filter::Scored> = request
                .candidates
                .iter()
                .zip(&self.notes)
                .map(|(candidate, &score)| mustard_core::domain::map_filter::Scored { id: candidate.id, score })
                .collect();
            let usage = mustard_core::domain::map_filter::FilterUsage {
                input_tokens: 21_000,
                millis: 40,
                cost_micro_usd: 882,
                model: "jev-1.13.0".to_string(),
            };
            Ok(Filtered { kept: mustard_core::domain::map_filter::cut(&scores, request.minimum), usage })
        }
    }

    impl FakeFilter {
        fn scoring(notes: &[f64]) -> Self {
            Self { asked: std::rc::Rc::default(), notes: notes.to_vec(), error: None }
        }

        fn failing(error: FilterError) -> Self {
            Self { asked: std::rc::Rc::default(), notes: Vec::new(), error: Some(error) }
        }

        /// A montagem que entrega este filtro, como a chave no projeto.
        fn assemble(&self) -> impl Fn(&Path, &mustard_core::ProjectConfig) -> Result<Assembled, FilterError> + '_ {
            move |_, _| Ok(Assembled { name: "jev", filter: Box::new(self.clone()), warning: None })
        }

        /// A montagem com a chave achada no projeto como a busca de verdade a
        /// acha, com `env` no lugar do ambiente de quem roda o teste e este
        /// filtro no lugar do serviço.
        fn assemble_from_the_project(
            &self,
            env: Option<&'static str>,
        ) -> impl Fn(&Path, &mustard_core::ProjectConfig) -> Result<Assembled, FilterError> + '_ {
            move |root, config| {
                let loaded = crate::shared::jev::key_in(root, config, env.map(str::to_string))?;
                Ok(Assembled { name: "jev", filter: Box::new(self.clone()), warning: loaded.warning })
            }
        }

        fn calls(&self) -> usize {
            self.asked.borrow().len()
        }

        fn last(&self) -> FilterRequest {
            self.asked.borrow().last().cloned().unwrap()
        }
    }

    /// Sem `search.filter` e sem chave, nem no ambiente nem no
    /// `mustard.json`, a busca é a do banco, igual à de antes do filtro, com
    /// o aviso da chave que falta uma vez por sessão; a frase de `--intent`
    /// não a muda. Com o filtro desligado, não há aviso.
    #[test]
    fn without_a_key_the_search_is_the_bank_one_and_warns_once_per_session() {
        let dir = search_project(FILTER_MAP, &json!({}));
        let fake = FakeFilter::scoring(&[0.9, 0.9, 0.9]);
        let bank = bank_answer(dir.path(), "cancelar");
        assert_eq!(bank["files"][0]["path"], json!("src/pedido.rs"), "{bank}");
        let opts = search_opts(dir.path(), "cancelar", None, Some("sessao-sem-chave"));
        let first = searched(&opts, &fake.assemble_from_the_project(None));
        let warned = mustard_core::translate("map.search.missing_key", Locale::PtBr);
        assert_eq!(first["warnings"], json!([warned]), "{first}");
        assert_eq!(without(&first, "warnings"), bank, "{first}");
        assert_eq!(searched(&opts, &fake.assemble_from_the_project(None)), bank, "the same session is not warned again");
        let with_intent = search_opts(dir.path(), "cancelar", Some("onde avisa o cliente"), Some("sessao-sem-chave"));
        assert_eq!(searched(&with_intent, &fake.assemble_from_the_project(None)), bank);
        assert_eq!(fake.calls(), 0);

        let off = search_project(FILTER_MAP, &json!({"filter": "none"}));
        let quiet = searched(&search_opts(off.path(), "cancelar", None, None), &fake.assemble_from_the_project(None));
        assert_eq!(quiet, bank_answer(off.path(), "cancelar"));
    }

    /// Com `search.filter: "none"`, a busca não monta o filtro, mesmo com a
    /// chave no projeto.
    #[test]
    fn the_filter_set_to_none_is_not_called_even_with_a_key() {
        let dir = search_project(FILTER_MAP, &json!({"filter": "none"}));
        let fake = FakeFilter::scoring(&[0.9, 0.9, 0.9]);
        let report = searched(&search_opts(dir.path(), "pedido", Some("onde o pedido é gravado"), None), &fake.assemble());
        assert_eq!(fake.calls(), 0);
        assert_eq!(report, bank_answer(dir.path(), "pedido"));
    }

    /// O filtro que dá nota a três candidatos faz a resposta trazer três
    /// peças, na ordem da nota, cada uma com o caminho, as linhas, o tipo, o
    /// nome, a assinatura, a primeira frase da documentação e a nota; nunca
    /// o corpo.
    #[test]
    fn three_scores_give_three_pieces_with_signature_and_doc_and_no_body() {
        let dir = search_project(FILTER_MAP, &json!({}));
        let intent = "onde o pedido é gravado";
        let project = crate::commands::spec_events::project(dir.path());
        let bank = map_search::candidates(&project.root, "pedido", intent, &project.languages, CANDIDATES).unwrap();
        assert_eq!(bank.candidates.len(), 4, "{bank:?}");
        // As notas vão aos três primeiros do banco, que também são o topo
        // dele: nada mais entra.
        let fake = FakeFilter::scoring(&[0.7, 0.9, 0.8]);
        let report = searched(&search_opts(dir.path(), "pedido", Some(intent), None), &fake.assemble());
        assert_eq!(fake.calls(), 1);
        assert_eq!(report["filter"], json!("jev"), "{report}");
        assert!(report.get("files").is_none(), "{report}");
        let pieces = report["pieces"].as_array().unwrap();
        let names: Vec<&str> = pieces.iter().map(|piece| piece["name"].as_str().unwrap()).collect();
        let order = [&bank.candidates[1], &bank.candidates[2], &bank.candidates[0]];
        assert_eq!(names, order.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), "{report}");
        assert_eq!(pieces.iter().map(|piece| piece["score"].clone()).collect::<Vec<_>>(), [json!(0.9), json!(0.8), json!(0.7)]);
        for piece in pieces {
            let keys: Vec<&str> = piece.as_object().unwrap().keys().map(String::as_str).collect();
            assert_eq!(keys, ["path", "line", "end_line", "kind", "name", "signature", "doc", "score"], "{piece}");
        }
        let saved = pieces.iter().find(|piece| piece["name"] == json!("gravar_pedido")).unwrap();
        assert_eq!(
            saved,
            &json!({"path": "src/pedido.rs", "line": 3, "end_line": 9, "kind": "function", "name": "gravar_pedido",
                    "signature": "pub fn gravar_pedido(pedido: &Pedido)", "doc": "Grava o pedido no banco.",
                    "score": saved["score"]})
        );
        assert!(!report.to_string().contains("trava a linha"), "the body never goes back: {report}");
    }

    /// O filtro que falha devolve a busca do banco, com o aviso do motivo
    /// uma vez por sessão; sem sessão conhecida, o aviso sai toda vez.
    #[test]
    fn a_failing_filter_answers_from_the_bank_and_warns_once_per_session() {
        let dir = search_project(FILTER_MAP, &json!({}));
        let fake = FakeFilter::failing(FilterError::Refused { status: 402 });
        let bank = bank_answer(dir.path(), "pedido");
        let opts = search_opts(dir.path(), "pedido", Some("onde o pedido é gravado"), Some("sessao-falha"));
        let first = searched(&opts, &fake.assemble());
        assert_eq!(fake.calls(), 1);
        assert_eq!(without(&first, "warnings"), bank, "{first}");
        let warned = mustard_core::translate("map.search.filter_failed", Locale::PtBr).replace("{reason}", "falta de crédito");
        assert_eq!(first["warnings"], json!([warned]), "{first}");
        let second = searched(&opts, &fake.assemble());
        assert_eq!(fake.calls(), 2);
        assert_eq!(second, bank, "the same session is not warned again");

        let unknown = search_opts(dir.path(), "pedido", None, None);
        for _ in 0..2 {
            assert_eq!(searched(&unknown, &fake.assemble())["warnings"], json!([warned]));
        }
    }

    /// Um nome de filtro que o projeto não conhece avisa e não filtra.
    #[test]
    fn an_unknown_filter_name_warns_and_does_not_filter() {
        let dir = search_project(FILTER_MAP, &json!({"filter": "outro"}));
        let fake = FakeFilter::scoring(&[0.9]);
        let report = searched(&search_opts(dir.path(), "pedido", None, Some("sessao-outro")), &fake.assemble());
        assert_eq!(fake.calls(), 0);
        let warned = mustard_core::translate("map.search.bad_filter", Locale::PtBr);
        assert_eq!(report["warnings"], json!([warned]), "{report}");
        assert_eq!(without(&report, "warnings"), bank_answer(dir.path(), "pedido"));
    }

    /// A frase ao filtro: a de `--intent`; sem ela, as palavras; e a palavra
    /// só vira o pedido de um pedaço de nome, no idioma do texto.
    #[test]
    fn a_single_word_without_intent_sends_the_name_piece_phrase() {
        let dir = search_project(FILTER_MAP, &json!({}));
        let fake = FakeFilter::scoring(&[0.9]);
        searched(&search_opts(dir.path(), "pedido", None, None), &fake.assemble());
        assert_eq!(fake.last().phrase, "pedaço de nome: pedido");
        assert_eq!(fake.last().words, ["pedido"]);
        searched(&search_opts(dir.path(), "gravar pedido", None, None), &fake.assemble());
        assert_eq!(fake.last().phrase, "gravar pedido");
        assert_eq!(fake.last().words, ["gravar", "pedido"]);
        searched(&search_opts(dir.path(), "pedido", Some("onde o pedido é gravado"), None), &fake.assemble());
        assert_eq!(fake.last().phrase, "onde o pedido é gravado");
    }

    /// Só a busca por assunto monta o filtro: nenhuma outra pergunta do mapa
    /// o pede, e os arquivos sugeridos à rodada e ao plano vêm do banco.
    #[test]
    fn only_the_search_question_assembles_the_filter() {
        let dir = tempdir().unwrap();
        store::write_text(dir.path(), EVERY_PART).unwrap();
        let skill_dir = dir.path().join("apps/rt/.claude/skills/add-pay");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill = skill_dir.join("SKILL.md");
        std::fs::write(&skill, "Veja `pay/index.rs`.\n").unwrap();
        let assembled = std::cell::Cell::new(0);
        let counting = |_: &Path, _: &mustard_core::ProjectConfig| {
            assembled.set(assembled.get() + 1);
            Err(FilterError::MissingKey)
        };
        for opts in every_question(dir.path(), &skill) {
            searched(&opts, &counting);
            assert_eq!(assembled.get(), 0, "{:?} assembled the filter", opts.question);
        }
        searched(&search_opts(dir.path(), "pagamento", None, None), &counting);
        assert_eq!(assembled.get(), 1);

        let languages = crate::commands::spec_events::project(dir.path()).languages;
        let bank: Vec<String> =
            map_search::search(dir.path(), "gravar pagamento", &languages, 3).unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(suggested_files(dir.path(), "gravar pagamento", 3, &languages), bank);
    }

    /// `search.candidates`: ausente, vale 100; com valor, vale o valor; com
    /// 0, vale 100 e avisa uma vez por sessão.
    #[test]
    fn the_candidates_setting_is_the_default_the_value_or_the_default_with_a_warning() {
        let map = many_orders(120);
        let sent = |search: Value, session: &str| {
            let dir = search_project(&map, &search);
            let fake = FakeFilter::scoring(&[0.9]);
            let opts = search_opts(dir.path(), "pedido", None, Some(session));
            let first = searched(&opts, &fake.assemble());
            let second = searched(&opts, &fake.assemble());
            (fake.last().candidates.len(), first["warnings"].clone(), second["warnings"].clone())
        };
        assert_eq!(sent(json!({}), "s1"), (100, Value::Null, Value::Null));
        assert_eq!(sent(json!({"candidates": 5}), "s2"), (5, Value::Null, Value::Null));
        let warned = mustard_core::translate("map.search.bad_number", Locale::PtBr)
            .replace("{key}", "candidates")
            .replace("{default}", "100");
        assert_eq!(sent(json!({"candidates": 0}), "s3"), (100, json!([warned]), Value::Null));
    }

    /// `search.cut_min` vai no pedido: ausente, 8; com valor, o valor; com
    /// 0, 8 e o aviso.
    #[test]
    fn the_cut_minimum_setting_goes_in_the_request() {
        let sent = |search: Value| {
            let dir = search_project(FILTER_MAP, &search);
            let fake = FakeFilter::scoring(&[0.9]);
            let report = searched(&search_opts(dir.path(), "pedido", None, None), &fake.assemble());
            (fake.last().minimum, report["warnings"].clone())
        };
        assert_eq!(sent(json!({})), (8, Value::Null));
        assert_eq!(sent(json!({"cut_min": 3})), (3, Value::Null));
        let warned = mustard_core::translate("map.search.bad_number", Locale::PtBr)
            .replace("{key}", "cut_min")
            .replace("{default}", "8");
        assert_eq!(sent(json!({"cut_min": 0})), (8, json!([warned])));
    }

    /// `search.max_returned`: ausente, a volta de 15 peças não se corta; com
    /// 5, ficam os 3 primeiros do banco e as 2 primeiras do corte; com 0,
    /// vale o padrão e avisa.
    #[test]
    fn the_max_returned_setting_caps_the_answer_or_warns() {
        let map = many_orders(40);
        // Os três primeiros do banco ficam abaixo do piso da nota; os doze
        // seguintes passam.
        let mut notes = vec![0.1; 3];
        notes.extend([0.9; 12]);
        let answered_with = |search: Value| {
            let dir = search_project(&map, &search);
            let fake = FakeFilter::scoring(&notes);
            searched(&search_opts(dir.path(), "pedido", None, None), &fake.assemble())
        };
        let whole = answered_with(json!({}));
        let pieces = whole["pieces"].as_array().unwrap();
        assert_eq!(pieces.len(), 15, "{whole}");
        assert!(whole.get("warnings").is_none(), "{whole}");

        let capped = answered_with(json!({"max_returned": 5}));
        let names = |report: &Value| -> Vec<String> {
            report["pieces"].as_array().unwrap().iter().map(|piece| piece["name"].as_str().unwrap().to_string()).collect()
        };
        let all = names(&whole);
        assert_eq!(names(&capped), [&all[0], &all[1], &all[12], &all[13], &all[14]].map(String::clone), "{capped}");

        let zero = answered_with(json!({"max_returned": 0}));
        assert_eq!(names(&zero), all);
        let warned = mustard_core::translate("map.search.bad_number", Locale::PtBr)
            .replace("{key}", "max_returned")
            .replace("{default}", "15");
        assert_eq!(zero["warnings"], json!([warned]), "{zero}");
    }

    /// Um projeto com a spec `spec` aberta, o checkout na branch dela e o
    /// mapa da busca com filtro.
    fn search_project_on(spec: &str) -> tempfile::TempDir {
        let dir = search_project(FILTER_MAP, &json!({}));
        crate::shared::spec_state::stand_on_spec_branch(dir.path(), spec);
        crate::commands::spec_events::write::record_open(dir.path(), spec, &format!("feature/{spec}"), "dev").unwrap();
        dir
    }

    fn calls_of(root: &Path, spec: &str) -> Vec<serde_json::Map<String, Value>> {
        use mustard_core::domain::spec_state::SpecState;
        crate::shared::spec_state::DiskSpecState::new(root)
            .log(spec)
            .map(|log| {
                log.visible().into_iter().filter(|e| e.event_type == "call").map(|e| e.fields.clone()).collect()
            })
            .unwrap_or_default()
    }

    /// A busca com o filtro, numa spec aberta, grava a chamada com o tempo,
    /// o filtro, o tempo dele, os tokens, o custo, os candidatos, o que
    /// voltou e o modelo que respondeu.
    #[test]
    fn a_filtered_search_in_an_open_spec_records_the_call_with_time_tokens_and_cost() {
        let dir = search_project_on("busca");
        let fake = FakeFilter::scoring(&[0.7, 0.9, 0.8]);
        searched(&search_opts(dir.path(), "pedido", Some("onde o pedido é gravado"), None), &fake.assemble());
        let calls = calls_of(dir.path(), "busca");
        assert_eq!(calls.len(), 1, "{calls:?}");
        let call = &calls[0];
        assert_eq!(call["command"], json!("map search"));
        assert_eq!(call["result"], json!("ok"));
        assert!(call["ms"].is_u64(), "{call:?}");
        assert_eq!(
            [&call["filter"], &call["filter_ms"], &call["tokens"], &call["cost_micro_usd"], &call["candidates"], &call["returned"], &call["model"]],
            [&json!("jev"), &json!(40), &json!(21_000), &json!(882), &json!(4), &json!(3), &json!("jev-1.13.0")]
        );
    }

    /// A busca sem filtro não grava chamada.
    #[test]
    fn a_search_without_the_filter_records_no_call() {
        let dir = search_project_on("sem-filtro");
        searched(&search_opts(dir.path(), "pedido", Some("onde o pedido é gravado"), None), &|_, _| Err(FilterError::MissingKey));
        assert!(calls_of(dir.path(), "sem-filtro").is_empty());
    }

    /// Uma chave falsa, que nenhuma saída pode mostrar.
    const PROJECT_KEY: &str = "tsk-falsa-9f8e7d6c5b4a";

    /// Grava `jev.key` com a chave falsa no `mustard.json` do projeto, com o
    /// resto do arquivo como estava.
    fn with_the_key(root: &Path) {
        let path = root.join("mustard.json");
        let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        config["jev"] = json!({"key": PROJECT_KEY});
        std::fs::write(&path, config.to_string()).unwrap();
    }

    /// Todo arquivo sob `.claude` do projeto, pelo texto.
    fn claude_texts(root: &Path) -> Vec<(PathBuf, String)> {
        let mut found = Vec::new();
        let mut pending = vec![root.join(".claude")];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    found.push((path.clone(), String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned()));
                }
            }
        }
        found
    }

    /// A chave em `jev.key` do `mustard.json`, fora do git, liga o filtro: a
    /// busca responde pelas notas, sem aviso. Nem a resposta, nem os avisos,
    /// nem a chamada gravada, nem arquivo nenhum sob `.claude` mostram a
    /// chave, com o filtro respondendo ou falhando.
    #[test]
    fn the_key_in_the_project_file_turns_the_filter_on_and_never_shows() {
        let dir = search_project_on("chave");
        with_the_key(dir.path());
        let fake = FakeFilter::scoring(&[0.7, 0.9, 0.8]);
        let opts = search_opts(dir.path(), "pedido", Some("onde o pedido é gravado"), Some("sessao-chave"));
        let report = searched(&opts, &fake.assemble_from_the_project(None));
        assert_eq!(fake.calls(), 1, "{report}");
        assert_eq!(report["filter"], json!("jev"), "{report}");
        assert!(report.get("warnings").is_none(), "{report}");
        assert!(!report.to_string().contains(PROJECT_KEY), "{report}");

        let failing = FakeFilter::failing(FilterError::Refused { status: 401 });
        let refused = searched(&opts, &failing.assemble_from_the_project(None));
        assert_eq!(failing.calls(), 1, "{refused}");
        assert!(refused.get("warnings").is_some(), "{refused}");
        assert!(!refused.to_string().contains(PROJECT_KEY), "{refused}");

        let calls = calls_of(dir.path(), "chave");
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert!(!format!("{calls:?}").contains(PROJECT_KEY), "{calls:?}");
        let texts = claude_texts(dir.path());
        assert!(!texts.is_empty());
        for (path, text) in texts {
            assert!(!text.contains(PROJECT_KEY), "{}", path.display());
        }
    }

    /// Um repositório git de verdade com o mapa do filtro e a chave falsa em
    /// `jev.key` do `mustard.json`, que o git guarda num commit.
    fn repo_with_the_key_in_git() -> tempfile::TempDir {
        let (dir, mut map) = scanned_repo(FILTER_MAP);
        let root = dir.path();
        with_the_key(root);
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(["commit", "-q", "-am", "chave"])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success(), "git commit: {}", String::from_utf8_lossy(&out.stderr));
        // A pasta do Mustard fica fora do git, como a instalação a deixa: a
        // marca do aviso dado na sessão não muda o conteúdo do projeto.
        std::fs::write(root.join(".git/info/exclude"), ".claude/\n").unwrap();
        let now = store::listing(root).unwrap();
        map["state"] = json!({"head": now.head, "listing": now.digest()});
        written_by_the_scan(root, &map);
        dir
    }

    /// A chave do `mustard.json` que o git guarda não se usa: sem a do
    /// ambiente, a busca é a do banco, com o aviso uma vez por sessão; com
    /// ela, o filtro vale, com o mesmo aviso. Nenhum dos dois mostra a
    /// chave.
    #[test]
    fn a_key_in_a_mustard_json_that_git_tracks_is_not_used_and_warns() {
        let dir = repo_with_the_key_in_git();
        let root = dir.path();
        let fake = FakeFilter::scoring(&[0.7, 0.9, 0.8]);
        let warned = mustard_core::translate("map.search.key_in_git", Locale::PtBr);
        let opts = search_opts(root, "pedido", None, Some("sessao-git"));
        let first = searched(&opts, &fake.assemble_from_the_project(None));
        assert_eq!(fake.calls(), 0, "{first}");
        assert_eq!(first["warnings"], json!([warned]), "{first}");
        assert_eq!(without(&first, "warnings"), bank_answer(root, "pedido"), "{first}");
        assert!(!first.to_string().contains(PROJECT_KEY), "{first}");
        let second = searched(&opts, &fake.assemble_from_the_project(None));
        assert!(second.get("warnings").is_none(), "the same session is not warned again: {second}");

        let with_env = search_opts(root, "pedido", None, Some("sessao-git-ambiente"));
        let filtered = searched(&with_env, &fake.assemble_from_the_project(Some("tsk-do-ambiente")));
        assert_eq!(fake.calls(), 1, "{filtered}");
        assert_eq!(filtered["filter"], json!("jev"), "{filtered}");
        assert_eq!(filtered["warnings"], json!([warned]), "{filtered}");
        assert!(!filtered.to_string().contains(PROJECT_KEY), "{filtered}");
    }

    /// A falha do filtro grava a chamada com o motivo junto do nome dele.
    #[test]
    fn a_failing_filter_records_the_reason_in_the_call() {
        let dir = search_project_on("falha");
        let fake = FakeFilter::failing(FilterError::Refused { status: 429 });
        searched(&search_opts(dir.path(), "pedido", None, None), &fake.assemble());
        let calls = calls_of(dir.path(), "falha");
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(calls[0]["filter"], json!("jev:busy"));
        assert!(calls[0].get("tokens").is_none(), "{calls:?}");
    }

    /// O mapa `map` gravado como a passada do scan o grava: com a marca dela
    /// em cada bloco e o índice nas línguas do projeto.
    fn written_by_the_scan(root: &Path, map: &Value) {
        let languages = crate::commands::spec_events::project(root).languages;
        store::save_at(&store::model_path(root), map, "scan 1", &languages).unwrap();
    }

    /// O scan de uma compilação mais velha abre o mapa: ele declara o bloco
    /// das declarações noutra versão, e o bloco perde a marca da passada. A
    /// abertura seguinte, na versão deste programa, o refaz vazio.
    fn opened_by_an_older_scan(root: &Path) {
        use mustard_core::io::map_db::{Block, Kind, MapDb};
        let older = Block { name: store::DECLS.name(), version: 1, tables: &[], schema: "", kind: Kind::Rebuilt(|_, _| Ok(())) };
        MapDb::open(&store::model_path(root), root, &[older]).unwrap();
    }

    /// Um repositório git de verdade, com o texto e o código em português e
    /// o mapa `map` gravado pela passada do scan no commit e no conteúdo de
    /// agora.
    fn scanned_repo(map: &str) -> (tempfile::TempDir, Value) {
        scanned_repo_with(map, &[])
    }

    /// [`scanned_repo`] com os arquivos `files`, pelo caminho e o texto, no
    /// commit.
    fn scanned_repo_with(map: &str, files: &[(&str, &str)]) -> (tempfile::TempDir, Value) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for (path, text) in files {
            let at = root.join(path);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(at, text).unwrap();
        }
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        std::fs::write(root.join("mustard.json"), json!({"language": {"text": "pt-BR", "code": "pt-BR"}}).to_string())
            .unwrap();
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "semente"]);
        let now = store::listing(root).unwrap();
        let mut map: Value = serde_json::from_str(map).unwrap();
        map["state"] = json!({"head": now.head, "listing": now.digest()});
        written_by_the_scan(root, &map);
        (dir, map)
    }

    fn no_history(_: &Path, _: &Path, _: &str, _: usize) -> mustard_core::platform::error::Result<HistoryReport> {
        panic!("a map without a base never reads a history")
    }

    /// A busca num mapa cujas declarações voltaram vazias numa troca de
    /// formato, sem passada do scan que as encha, recusa com a saída: não
    /// chama o filtro pago nem grava chamada, que entraria na conta do mês
    /// sem nenhum candidato.
    #[test]
    fn a_search_on_declarations_emptied_by_a_format_change_is_refused_and_records_no_call() {
        let dir = search_project_on("zerada");
        let root = dir.path();
        written_by_the_scan(root, &serde_json::from_str(FILTER_MAP).unwrap());
        opened_by_an_older_scan(root);
        let fake = FakeFilter::scoring(&[0.9]);
        let report = searched(&search_opts(root, "pedido", Some("onde o pedido é gravado"), None), &fake.assemble());
        assert_eq!(report["ok"], json!(false), "{report}");
        assert_eq!(report["reason"], json!("map-unfilled"), "{report}");
        let refusal = MapRefusal::MapUnfilled { blocks: vec!["decls".to_string()] };
        assert_eq!(report["hint"], json!(refusal.message(Locale::PtBr)), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("mustard-rt run scan"), "{report}");
        assert_eq!(fake.calls(), 0, "the paid filter is not called");
        assert!(calls_of(root, "zerada").is_empty(), "no call enters the month's count");
    }

    /// Com o scan da mesma compilação, a pergunta relê o mapa cujas
    /// declarações voltaram vazias, mesmo sem commit nem arquivo novo, e a
    /// busca com filtro manda os 100 candidatos, não nenhum.
    #[test]
    fn declarations_emptied_by_a_format_change_are_read_again_and_the_search_sends_its_hundred_candidates() {
        let (dir, map) = scanned_repo(&many_orders(120));
        let root = dir.path();
        opened_by_an_older_scan(root);
        let passes = std::cell::Cell::new(0);
        let same_build = |root: &Path, _: &Path| {
            passes.set(passes.get() + 1);
            written_by_the_scan(root, &map);
            Ok(ScanReport::default())
        };
        let fake = FakeFilter::scoring(&[0.9]);
        let report = super::map_at(&search_opts(root, "pedido", None, None), &same_build, &no_history, &fake.assemble());
        assert_eq!(passes.get(), 1, "{report}");
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(fake.last().candidates.len(), 100, "{report}");
    }

    /// Com o scan de outra compilação, as perguntas que leem as declarações
    /// — quem usa, o trecho, os exemplos e a história — recusam como a
    /// busca, com a saída, em vez de responder que a declaração não existe;
    /// quem importa e o resumo, que não as leem, respondem.
    #[test]
    fn a_scan_from_another_build_leaves_the_questions_on_declarations_refused_instead_of_empty() {
        let (dir, map) = scanned_repo(FILTER_MAP);
        let root = dir.path();
        opened_by_an_older_scan(root);
        let older_build = |root: &Path, _: &Path| {
            written_by_the_scan(root, &map);
            opened_by_an_older_scan(root);
            Ok(ScanReport::default())
        };
        let fake = FakeFilter::scoring(&[0.9]);
        let on = |question, file: Option<&str>, name: Option<&str>| MapOpts {
            file: file.map(str::to_string),
            name: name.map(str::to_string),
            ..ask(root, question)
        };
        for opts in [
            on(Question::Users, None, Some("gravar_pedido")),
            on(Question::Slice, Some("src/pedido.rs"), Some("gravar_pedido")),
            on(Question::Examples, Some("src/pedido.rs"), None),
            on(Question::History, Some("src/pedido.rs"), Some("gravar_pedido")),
        ] {
            let report = super::map_at(&opts, &older_build, &no_history, &fake.assemble());
            assert_eq!(report["reason"], json!("map-unfilled"), "{:?}: {report}", opts.question);
        }
        for opts in [on(Question::Importers, Some("src/pedido.rs"), None), on(Question::Summary, None, None)] {
            let report = super::map_at(&opts, &older_build, &no_history, &fake.assemble());
            assert_eq!(report["ok"], json!(true), "{:?}: {report}", opts.question);
        }
    }

    /// Com o scan de outra compilação, que grava as declarações no formato
    /// dele, a pergunta relê o mapa uma vez e a busca recusa, com a saída,
    /// em vez de responder vazio.
    #[test]
    fn a_scan_from_another_build_leaves_the_search_refused_instead_of_empty() {
        let (dir, map) = scanned_repo(FILTER_MAP);
        let root = dir.path();
        opened_by_an_older_scan(root);
        let passes = std::cell::Cell::new(0);
        let older_build = |root: &Path, _: &Path| {
            passes.set(passes.get() + 1);
            written_by_the_scan(root, &map);
            opened_by_an_older_scan(root);
            Ok(ScanReport::default())
        };
        let fake = FakeFilter::scoring(&[0.9]);
        let opts = search_opts(root, "pedido", Some("onde o pedido é gravado"), None);
        let report = super::map_at(&opts, &older_build, &no_history, &fake.assemble());
        assert_eq!(passes.get(), 1, "{report}");
        assert_eq!(report["reason"], json!("map-unfilled"), "{report}");
        assert_eq!(fake.calls(), 0, "{report}");
    }

    /// O arquivo de `src/pedido.rs` no commit: `gravar_pedido` nas linhas 3
    /// a 5, como o mapa diz.
    const ORDER_FILE: &str = "// pedidos\n\npub fn gravar_pedido(pedido: &Pedido) {\n    banco::gravar(pedido);\n}\n\npub fn cancelar_pedido(id: u64) {}\n";

    /// O mapa de `src/pedido.rs` com as linhas do commit.
    const ORDER_MAP: &str = r#"{"modules": [
      {"path": "src/pedido.rs", "loc": 8, "declarations": [
        {"kind": "function", "name": "gravar_pedido", "line": 3, "end_line": 5,
         "signature": "pub fn gravar_pedido(pedido: &Pedido)", "doc": ""},
        {"kind": "function", "name": "cancelar_pedido", "line": 7, "end_line": 7,
         "signature": "pub fn cancelar_pedido(id: u64)", "doc": ""}]}
    ]}"#;

    /// O trecho perguntado de dentro de uma cópia de trabalho do projeto,
    /// como a de uma onda, sai do arquivo da cópia, não do projeto: com duas
    /// linhas novas no topo, `gravar_pedido` sai inteira nas linhas 5 a 7 da
    /// cópia. A declaração que a cópia mudou recusa e manda ler o arquivo
    /// da cópia por faixa de linhas. Perguntado no projeto, o trecho segue o
    /// do projeto, nas linhas do mapa.
    #[test]
    fn the_slice_asked_inside_a_working_copy_reads_the_copy_file() {
        let (dir, map) = scanned_repo_with(ORDER_MAP, &[("src/pedido.rs", ORDER_FILE)]);
        let root = dir.path();
        let copies = tempdir().unwrap();
        let copy = copies.path().join("c");
        let out = std::process::Command::new("git")
            .args(["worktree", "add", "-q", "-b", "onda"])
            .arg(&copy)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success(), "git worktree: {}", String::from_utf8_lossy(&out.stderr));
        let rescan = |root: &Path, _: &Path| {
            written_by_the_scan(root, &map);
            Ok(ScanReport::default())
        };
        let slice_from = |start: &Path, name: &str| {
            let opts = MapOpts {
                file: Some("src/pedido.rs".to_string()),
                name: Some(name.to_string()),
                ..ask(start, Question::Slice)
            };
            super::map_at(&opts, &rescan, &no_history, &|_, _| Err(FilterError::MissingKey))
        };
        let declaration = "pub fn gravar_pedido(pedido: &Pedido) {\n    banco::gravar(pedido);\n}";
        let same = slice_from(&copy, "gravar_pedido");
        assert_eq!((&same["line"], &same["end_line"]), (&json!(3), &json!(5)), "{same}");
        assert_eq!(same["slice"], json!(declaration), "{same}");

        std::fs::write(copy.join("src/pedido.rs"), format!("use banco;\nuse pedido::Pedido;\n{ORDER_FILE}")).unwrap();
        let moved = slice_from(&copy.join("src"), "gravar_pedido");
        assert_eq!(moved["ok"], json!(true), "{moved}");
        assert_eq!((&moved["line"], &moved["end_line"]), (&json!(5), &json!(7)), "{moved}");
        assert_eq!(moved["slice"], json!(declaration), "{moved}");

        let changed = ORDER_FILE.replace("banco::gravar(pedido);", "banco::gravar(pedido)?;");
        std::fs::write(copy.join("src/pedido.rs"), format!("use banco;\n{changed}")).unwrap();
        let refused = slice_from(&copy, "gravar_pedido");
        assert_eq!(refused["reason"], json!("changed-in-copy"), "{refused}");
        let hint = refused["hint"].as_str().unwrap();
        assert!(hint.contains("gravar_pedido") && hint.contains("src/pedido.rs"), "{refused}");
        let untouched = slice_from(&copy, "cancelar_pedido");
        assert_eq!((&untouched["line"], &untouched["end_line"]), (&json!(8), &json!(8)), "{untouched}");
        assert_eq!(untouched["slice"], json!("pub fn cancelar_pedido(id: u64) {}"), "{untouched}");

        let in_the_project = slice_from(root, "gravar_pedido");
        assert_eq!((&in_the_project["line"], &in_the_project["end_line"]), (&json!(3), &json!(5)), "{in_the_project}");
        assert_eq!(in_the_project["slice"], json!(declaration), "{in_the_project}");
    }
}
