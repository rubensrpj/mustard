//! `map_triage` — a triagem da busca do mapa: o grau da resposta e a busca
//! funda.
//!
//! Os arquivos da resposta saem da ordem única ([`crate::io::map_order`]):
//! a busca dos arquivos ([`map_search::ranked_files`]) somada à lista das
//! declarações que vai ao filtro. Os sinais do grau saem da busca dos
//! arquivos, que responde pelos campos fortes do índice: o nome (o do
//! arquivo, o das declarações), a assinatura, as mensagens de erro e de log,
//! os rótulos de tela e as rotas. A triagem lê dela três sinais — a nota do
//! primeiro achado, quantas palavras da pergunta ele traz em campo forte e a
//! distância dele para o segundo — e dá o grau de 0 a 5
//! ([`crate::domain::triage`]).
//!
//! Do grau 3 para baixo, cada palavra da pergunta que o primeiro achado não
//! traz em campo forte é procurada nos campos fracos, um degrau por vez: os
//! comentários, os testes, as mensagens de commit e o glossário do mapa. O
//! achado volta ao dono num passo só, sem encadear: o comentário, à
//! declaração que o contém; o teste, aos arquivos que ele cobre pela ligação
//! `tests` do mapa; o commit, aos arquivos que ele mudou; a marca do
//! glossário, à declaração que ela aponta. Cada dono aparece uma vez, com o
//! que o achou e se a ligação é provada ou suspeita, e o teste que fala da
//! mesma palavra que uma função vira parte da entrada dela. O achado muito
//! mais fraco que o primeiro fica fora.
//!
//! Sem achado nenhum, nem nos campos fortes nem na busca funda, o grau é 0.
//!
//! **O sentido.** O grau, a marca e o "cravado" saem sempre da resposta das
//! palavras que a pergunta escreve. Só a resposta que não fica cravada lê
//! também as palavras vizinhas ([`crate::io::map_sense`]): o `apagar` que
//! acha o projeto que só escreve `remove`, o `parcela` que acha
//! `splitInstallments`. O "não achei" só cai quando as palavras, as vizinhas
//! entre elas, acham algum arquivo; a ordem dos vetores sozinha nunca
//! desfaz esse aviso.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::domain::ast::{is_test_path, tested_name};
use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::project_map::{Found, MapRefusal};
use crate::domain::ranking::{idf_x1024, SCALE};
use crate::domain::search::{folded_name, TOP};
use crate::domain::triage::{self, Lead, Signals};
use crate::io::map_check;
use crate::io::map_glossary::{self, Learned};
use crate::io::map_order;
use crate::io::map_sense::Sense;
use crate::io::map_words::{question, Word};
use crate::io::map_search::{add_texts, by_fields, indexed, text};
use crate::io::project_map::{model_path, unreadable, SEARCHED};
use crate::platform::error::Result;

/// Os campos do nível dos arquivos que contam como fortes: o nome do arquivo
/// e das pastas, os nomes que ele declara, as mensagens de log e de erro e
/// os outros textos, onde entram os rótulos de tela e as rotas.
const STRONG_FILE: [&str; 5] = ["name", "path", "log", "error", "text"];

/// Os campos fortes do nível das declarações: os mesmos, mais a assinatura.
const STRONG_DECL: [&str; 6] = ["name", "path", "signature", "log", "error", "text"];

/// Os campos dos comentários no nível das declarações: a documentação, a
/// inteira e os comentários escritos no corpo.
const COMMENT_DECL: [&str; 3] = ["doc", "whole_doc", "body_comment"];

/// Os campos dos comentários no nível dos arquivos: os do começo e os que o
/// scan guardou fora das declarações.
const COMMENT_FILE: [&str; 2] = ["file_doc", "file_comment"];

/// Todos os campos do nível dos arquivos.
const FILE_FIELDS: [&str; 8] = ["name", "path", "doc", "log", "error", "text", "file_doc", "file_comment"];

/// A pergunta de uma palavra só procura também o pedaço do nome, a partir
/// deste tamanho: o mesmo da busca dos arquivos.
const PIECE_MIN_CHARS: usize = 4;

/// Por onde a busca funda achou uma palavra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Via {
    /// Um comentário escrito na declaração.
    Comment,
    /// Um teste, com o caminho do arquivo dele.
    Test(String),
    /// Um commit, com o título.
    Commit(String),
    /// Uma marca do glossário: a palavra que uma edição já ligou a ela.
    Glossary,
}

/// A ligação entre o que achou a palavra e o dono dela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Link {
    /// O achado está no dono ou foi ligado a ele por uma edição, por um
    /// import ou pelo nome do teste.
    Proven,
    /// O achado só aponta para o dono: o teste que muda com ele, o commit
    /// que o tocou.
    Suspected,
}

/// A declaração dona de um achado.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Located {
    pub kind: String,
    pub name: String,
    pub line: u64,
    pub end_line: u64,
}

/// Um dono achado pela busca funda: o arquivo, a declaração quando o achado
/// chega a ela, as palavras que ele responde, por onde vieram e se a ligação
/// é provada ou suspeita.
#[derive(Debug, Clone, PartialEq)]
pub struct Deeper {
    pub path: String,
    pub decl: Option<Located>,
    /// As palavras da pergunta, quebradas, que o achado responde.
    pub words: Vec<String>,
    pub via: Vec<Via>,
    /// `Proven` quando alguma das ligações é provada.
    pub link: Link,
    /// A nota da entrada: cada palavra vale o da melhor ligação dela.
    pub score: f64,
}

/// A resposta triada: o grau, os sinais de onde ele saiu, as palavras da
/// pergunta quebradas, os arquivos dos campos fortes e, do grau 3 para
/// baixo, o que a busca funda achou.
#[derive(Debug, Clone, PartialEq)]
pub struct Triaged {
    pub grade: u8,
    pub signals: Signals,
    /// As palavras da pergunta, quebradas e sem as de ligação.
    pub words: Vec<String>,
    /// As palavras da pergunta que o primeiro achado não traz em campo forte:
    /// todas, sem achado nenhum.
    pub missing: Vec<String>,
    pub files: Vec<Found>,
    pub deeper: Vec<Deeper>,
    /// O que a conferência dos primeiros candidatos diz da frente da lista.
    pub lead: Lead,
}

impl Triaged {
    /// A marca da resposta: cravado, parcial ou não achou
    /// ([`triage::mark`]).
    #[must_use]
    pub fn mark(&self) -> triage::Mark {
        triage::mark(self.grade, self.lead)
    }
}

/// A busca dos arquivos do mapa do projeto em `root`, triada: as recusas são
/// as de [`crate::io::map_search::search`]. Os arquivos saem na ordem única
/// das palavras de `query` e da frase de `intent` ([`crate::io::map_order`]),
/// a mesma da lista de candidatos do filtro.
pub fn triage(
    root: &Path,
    (query, intent): (&str, &str),
    languages: &Languages,
    limit: usize,
) -> std::result::Result<Triaged, MapRefusal> {
    triage_in(&model_path(root), Some(root), (query, intent), languages, limit)
}

/// A triagem de [`triage`] no mapa gravado em `model`.
pub fn triage_at(
    model: &Path,
    (query, intent): (&str, &str),
    languages: &Languages,
    limit: usize,
) -> std::result::Result<Triaged, MapRefusal> {
    triage_in(model, map_check::root_of(model), (query, intent), languages, limit)
}

/// A triagem no mapa gravado em `model`, com o corpo das declarações lido do
/// disco a partir de `root`; sem ele, a conferência lê só o mapa.
fn triage_in(
    model: &Path,
    root: Option<&Path>,
    (query, intent): (&str, &str),
    languages: &Languages,
    limit: usize,
) -> std::result::Result<Triaged, MapRefusal> {
    let db = indexed(model, languages, &SEARCHED)?;
    triaged(db.conn(), root, (query, intent), languages, limit, false).map_err(unreadable)
}

/// A triagem sobre o banco aberto. Com `whole`, a busca funda roda em
/// qualquer grau e traz tudo o que achou, sem o corte do muito mais fraco e
/// sem o limite: a medida da régua precisa dela inteira.
///
/// Os arquivos são os `limit` primeiros da ordem única. Os sinais do grau
/// ficam os do banco, a nota do primeiro e do segundo achado dele: quando a
/// ordem única põe outro arquivo na frente, o grau não passa de 4 e a marca
/// nunca é cravado, porque a chance fala do primeiro do banco. Achado só da
/// lista, sem nenhum do banco, tem o grau mais fraco.
fn triaged(
    conn: &Connection,
    root: Option<&Path>,
    (query, intent): (&str, &str),
    languages: &Languages,
    limit: usize,
    whole: bool,
) -> Result<Triaged> {
    let mut normalizer = Normalizer::new(languages);
    let words = question(conn, &mut normalizer, query)?;
    let check = map_order::Check::On(root);
    // A resposta de sempre, só das palavras escritas: dela saem o grau, a
    // marca e o cravado, e é ela que o cravado entrega.
    let ordered = map_order::ordered_with(conn, check, &Sense::off(), query, intent, languages)?;
    let lead = ordered.lead;
    let strong = strong_words(conn, query, &words, ordered.files.first())?;
    let scale = |score: u64| score as f64 / 1024.0;
    let signals = Signals {
        words: words.len(),
        strong: strong.iter().filter(|hit| **hit).count(),
        first: ordered.bank.first().map(|file| scale(file.score)),
        second: ordered.bank.get(1).map(|file| scale(file.score)),
    };
    let mut grade = triage::grade(&signals);
    let agrees = ordered.files.first().map(|file| &file.path) == ordered.bank.first().map(|file| &file.path);
    if !agrees {
        grade = grade.min(triage::UNSURE_GRADE);
    }
    // A resposta que não é cravada lê também o sentido: as palavras vizinhas
    // e a ordem dos vetores. O "não achei" só cai quando as palavras, as
    // vizinhas entre elas, acham algum arquivo; a ordem dos vetores sozinha
    // nunca acha o que as palavras não acham.
    let mut found = !ordered.files.is_empty();
    let mut files = ordered.files;
    if triage::mark(grade, lead) != triage::Mark::Pinned {
        let sense = Sense::read(conn, languages, (query, intent), false)?;
        if sense.changes_words() {
            let sensed = map_order::ordered_with(conn, check, &sense, query, intent, languages)?;
            found = found || !sensed.files.is_empty();
            files = sensed.files;
        }
    }
    let mut files: Vec<Found> = files.into_iter().take(limit).collect();
    add_texts(conn, query, languages, &mut files)?;
    let missing: Vec<&Word> = words.iter().zip(&strong).filter(|(_, hit)| !**hit).map(|(word, _)| word).collect();
    let mut deeper = Vec::new();
    if grade <= triage::DEEP_UNTIL || whole {
        deeper = search_deeper(conn, &mut normalizer, &missing, (!whole).then_some(TOP))?;
    }
    if grade == 0 && (!deeper.is_empty() || found) {
        grade = 1;
    }
    let missing: Vec<String> = missing.into_iter().map(|word| word.plain.clone()).collect();
    Ok(Triaged { grade, signals, words: words.into_iter().map(|word| word.plain).collect(), missing, files, deeper, lead })
}

/// De cada palavra da pergunta, se o primeiro achado a traz em campo forte:
/// nos campos fortes do arquivo ou nos das declarações dele. Na pergunta de
/// uma palavra só que procura o pedaço do nome, o nome de uma declaração do
/// arquivo que traz a palavra também conta.
fn strong_words(conn: &Connection, query: &str, words: &[Word], first: Option<&Found>) -> Result<Vec<bool>> {
    let Some(first) = first else { return Ok(vec![false; words.len()]) };
    let file: Option<i64> =
        conn.query_row("SELECT rowid FROM files WHERE path = ?1", [&first.path], |row| row.get(0)).ok();
    let Some(file) = file else { return Ok(vec![false; words.len()]) };
    let mut stmt = conn.prepare("SELECT rowid, name FROM decls WHERE file = ?1")?;
    let decls: Vec<(i64, String)> = stmt
        .query_map([&first.path], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let ids: HashSet<i64> = decls.iter().map(|(id, _)| *id).collect();
    let piece = query.trim();
    let piece = (piece.split_whitespace().count() == 1 && piece.chars().count() >= PIECE_MIN_CHARS)
        .then(|| folded_name(piece));
    let mut out = Vec::with_capacity(words.len());
    for word in words {
        let in_file = occurrences(conn, "file_vocab", &word.indexed)?
            .iter()
            .any(|(doc, col)| *doc == file && STRONG_FILE.contains(&col.as_str()));
        let in_decl = || -> Result<bool> {
            Ok(occurrences(conn, "decl_vocab", &word.indexed)?
                .iter()
                .any(|(doc, col)| ids.contains(doc) && STRONG_DECL.contains(&col.as_str())))
        };
        let in_piece = || piece.as_ref().is_some_and(|piece| decls.iter().any(|(_, name)| folded_name(name).contains(piece)));
        out.push(in_file || in_piece() || in_decl()?);
    }
    Ok(out)
}

/// Cada ocorrência de uma das formas em uma tabela de palavras do índice: o
/// número do documento e a coluna, sem repetição.
fn occurrences(conn: &Connection, vocab: &str, forms: &[String]) -> Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(&format!("SELECT doc, col FROM {vocab} WHERE term = ?1"))?;
    let mut out: Vec<(i64, String)> = Vec::new();
    for form in forms {
        let mut rows = stmt.query([form])?;
        while let Some(row) = rows.next()? {
            out.push((row.get(0)?, text(row, 1)?));
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Cada achado dos degraus da busca funda, antes de juntar por dono.
struct Evidence {
    /// As palavras de `missing` que ele responde.
    words: Vec<usize>,
    via: Via,
    link: Link,
    /// A nota do achado no degrau dele.
    score: f64,
}

/// O dono de um achado: o arquivo e, quando o achado chega a ela, a
/// declaração.
type Owner = (String, Option<Located>);

/// Quantos achados cada degrau leva para a junção por dono: a resposta leva
/// [`TOP`] dos donos, e o achado do degrau que ficou abaixo desses não muda
/// a resposta.
const PER_STEP: usize = 30;

/// O que a busca funda lê do índice sobre cada palavra que falta: as
/// ocorrências dela nas tabelas de palavras dos dois níveis e o peso dela.
struct Look {
    decl: Vec<(i64, String)>,
    file: Vec<(i64, String)>,
    /// O inverso da frequência da palavra entre as declarações do índice,
    /// em todos os campos. A palavra que quase toda declaração traz pesa
    /// perto de zero, e a rara pesa muito.
    rarity: f64,
}

/// O que cada degrau lê: o banco, as palavras que faltam com o que o índice
/// diz delas e o caminho de cada arquivo do código.
struct Ground<'a> {
    conn: &'a Connection,
    missing: &'a [&'a Word],
    looks: Vec<Look>,
    paths: HashMap<i64, String>,
}

impl Ground<'_> {
    /// As formas de cada palavra que falta, para a nota do nível.
    fn forms(&self) -> Vec<Vec<String>> {
        self.missing.iter().map(|word| word.forms.clone()).collect()
    }

    /// As palavras que falta a que o documento `doc` responde numa das
    /// colunas `cols` do nível das declarações (`decl`) ou dos arquivos.
    fn covered(&self, decl: bool, doc: i64, cols: &[&str]) -> Vec<usize> {
        (0..self.missing.len())
            .filter(|&at| {
                let list = if decl { &self.looks[at].decl } else { &self.looks[at].file };
                list.iter().any(|(id, col)| *id == doc && cols.contains(&col.as_str()))
            })
            .collect()
    }
}

/// A busca funda das palavras `missing`, que o primeiro achado não traz em
/// campo forte: os degraus, o dono de cada achado, as entradas juntadas por
/// dono, a nota de cada uma e o corte do que é muito mais fraco que a
/// primeira, até `limit` entradas. Sem `limit`, tudo o que achou, sem o corte.
fn search_deeper(
    conn: &Connection,
    normalizer: &mut Normalizer,
    missing: &[&Word],
    limit: Option<usize>,
) -> Result<Vec<Deeper>> {
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let docs: i64 = conn.query_row("SELECT count(*) FROM decl_lengths", [], |row| row.get(0))?;
    let docs = usize::try_from(docs).unwrap_or(0);
    let mut looks = Vec::with_capacity(missing.len());
    for word in missing {
        let decl = occurrences(conn, "decl_vocab", &word.indexed)?;
        let file = occurrences(conn, "file_vocab", &word.indexed)?;
        let seen: HashSet<i64> = decl.iter().map(|(doc, _)| *doc).collect();
        looks.push(Look { decl, file, rarity: idf_x1024(seen.len(), docs) as f64 / SCALE as f64 });
    }
    let ground = Ground { conn, missing, looks, paths: file_paths(conn)? };
    let mut found: BTreeMap<Owner, Vec<Evidence>> = BTreeMap::new();
    let mut add = |owner: Owner, evidence: Evidence| found.entry(owner).or_default().push(evidence);
    comments(&ground, &mut add)?;
    tests(&ground, &mut add)?;
    commits(&ground, normalizer, &mut add)?;
    glossary(&ground, &mut add)?;
    Ok(entries(merged(found), missing, limit))
}

/// O caminho de cada arquivo do código do índice, pelo número dele.
fn file_paths(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut stmt = conn.prepare("SELECT rowid, path FROM files WHERE file_class IS NULL OR file_class = ''")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// A declaração de número `id` do nível das declarações.
fn located(conn: &Connection, id: i64) -> Result<Option<(String, Located)>> {
    let mut stmt = conn.prepare("SELECT file, kind, name, line, end_line FROM decls WHERE rowid = ?1")?;
    let mut rows = stmt.query([id])?;
    let Some(row) = rows.next()? else { return Ok(None) };
    let number = |at: usize| -> Result<u64> { Ok(row.get::<_, Option<i64>>(at)?.unwrap_or(0).max(0) as u64) };
    let (line, end) = (number(3)?, number(4)?);
    Ok(Some((text(row, 0)?, Located { kind: text(row, 1)?, name: text(row, 2)?, line, end_line: end.max(line) })))
}

/// Os `PER_STEP` primeiros da lista de notas, da maior para a menor.
fn best(mut scores: Vec<(i64, f64)>) -> Vec<(i64, f64)> {
    scores.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scores.truncate(PER_STEP);
    scores
}

/// Primeiro degrau: o comentário volta à declaração que o contém — a mais
/// interna, quando o de uma declaração de fora também traz o comentário — e,
/// fora de toda declaração, ao arquivo. O comentário está no dono: a ligação
/// é provada.
fn comments(ground: &Ground<'_>, add: &mut impl FnMut(Owner, Evidence)) -> Result<()> {
    let conn = ground.conn;
    let forms = ground.forms();
    let mut hits: Vec<(i64, bool, String, Located, f64)> = Vec::new();
    for (doc, score) in best(by_fields(conn, true, &COMMENT_DECL, &forms)?) {
        let Some((path, place)) = located(conn, doc)? else { continue };
        let own = !ground.covered(true, doc, &["doc"]).is_empty();
        hits.push((doc, own, path, place, score));
    }
    let inside = |outer: &Located, inner: &Located| {
        inner != outer && inner.line >= outer.line && inner.end_line <= outer.end_line
    };
    let mut in_decls: HashSet<&String> = HashSet::new();
    for (doc, own, path, place, score) in &hits {
        let nested = hits.iter().any(|(_, _, other, inner, _)| other == path && inside(place, inner));
        if !own && nested {
            continue;
        }
        in_decls.insert(path);
        add(
            (path.clone(), Some(place.clone())),
            Evidence {
                words: ground.covered(true, *doc, &COMMENT_DECL),
                via: Via::Comment,
                link: Link::Proven,
                score: *score,
            },
        );
    }
    for (doc, score) in best(by_fields(conn, false, &COMMENT_FILE, &forms)?) {
        let Some(path) = ground.paths.get(&doc).filter(|path| !is_test_path(path) && !in_decls.contains(*path)) else {
            continue;
        };
        add(
            (path.clone(), None),
            Evidence { words: ground.covered(false, doc, &COMMENT_FILE), via: Via::Comment, link: Link::Proven, score },
        );
    }
    Ok(())
}

/// O que o mapa guarda da ligação entre arquivos: as importações e os testes
/// de cada um.
struct Graph {
    deps: HashMap<String, Vec<String>>,
    /// De cada arquivo, os testes que o cobrem.
    tests: HashMap<String, Vec<String>>,
}

impl Graph {
    fn read(conn: &Connection) -> Result<Self> {
        let mut stmt = conn.prepare("SELECT path, deps, tests FROM links")?;
        let mut rows = stmt.query([])?;
        let (mut deps, mut tests) = (HashMap::new(), HashMap::new());
        let list = |json: Option<String>| -> Vec<String> {
            json.and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default()
        };
        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            deps.insert(path.clone(), list(row.get(1)?));
            tests.insert(path, list(row.get(2)?));
        }
        Ok(Self { deps, tests })
    }
}

/// Segundo degrau: o teste volta aos arquivos que ele cobre, pela ligação
/// `tests` do mapa. A ligação é provada quando o teste importa o arquivo ou
/// leva o nome dele, e suspeita quando o teste só o usa ou muda junto com
/// ele: a nota da suspeita se reparte entre os arquivos que o teste cobre.
fn tests(ground: &Ground<'_>, add: &mut impl FnMut(Owner, Evidence)) -> Result<()> {
    let every: Vec<&str> = FILE_FIELDS.to_vec();
    let mut scored: Vec<(i64, f64)> = by_fields(ground.conn, false, &every, &ground.forms())?
        .into_iter()
        .filter(|(doc, _)| ground.paths.get(doc).is_some_and(|path| is_test_path(path)))
        .collect();
    scored = best(std::mem::take(&mut scored));
    if scored.is_empty() {
        return Ok(());
    }
    let graph = Graph::read(ground.conn)?;
    for (doc, score) in scored {
        let Some(test) = ground.paths.get(&doc) else { continue };
        let words = ground.covered(false, doc, &every);
        let mut covered: Vec<&String> =
            graph.tests.iter().filter(|(_, tests)| tests.contains(test)).map(|(path, _)| path).collect();
        covered.sort();
        let proven = |file: &str| {
            graph.deps.get(test).is_some_and(|deps| deps.iter().any(|dep| dep == file))
                || tested_name(test)
                    .zip(Path::new(file).file_stem().and_then(|stem| stem.to_str()))
                    .is_some_and(|(tested, stem)| tested.eq_ignore_ascii_case(stem))
        };
        let suspected = covered.iter().filter(|file| !proven(file)).count();
        for file in covered {
            let (link, share) = if proven(file) { (Link::Proven, 1.0) } else { (Link::Suspected, 1.0 / suspected as f64) };
            add(
                (file.clone(), None),
                Evidence { words: words.clone(), via: Via::Test(test.clone()), link, score: score * share },
            );
        }
    }
    Ok(())
}

/// Terceiro degrau: o commit volta aos arquivos do código que ele criou ou
/// mudou, do mais novo ao mais velho. O commit só cita o arquivo: a ligação
/// é suspeita, e a nota se reparte entre os arquivos do commit, que dizem
/// menos de cada um quanto mais são. O mapa sem a história não dá nada.
fn commits(ground: &Ground<'_>, normalizer: &mut Normalizer, add: &mut impl FnMut(Owner, Evidence)) -> Result<()> {
    let conn = ground.conn;
    let mut stmt = conn.prepare("SELECT path FROM history_paths ORDER BY rowid")?;
    let listed: Vec<Option<String>> = stmt.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
    if listed.is_empty() {
        return Ok(());
    }
    let code: HashSet<&String> = ground.paths.values().collect();
    let mut stmt = conn.prepare("SELECT title, added, changed FROM commits ORDER BY at DESC, rowid")?;
    let mut rows = stmt.query([])?;
    let mut found: Vec<(String, Evidence)> = Vec::new();
    while let Some(row) = rows.next()? {
        let title = text(row, 0)?;
        let title_forms: HashSet<String> = normalizer.forms(&title).into_iter().flatten().collect();
        let words: Vec<usize> = (0..ground.missing.len())
            .filter(|&at| ground.missing[at].forms.iter().any(|form| title_forms.contains(form)))
            .collect();
        if words.is_empty() {
            continue;
        }
        let touched: HashSet<usize> = [1, 2]
            .into_iter()
            .map(|at| Ok(serde_json::from_str::<Vec<usize>>(&text(row, at)?).unwrap_or_default()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        let mut files: Vec<&String> = touched
            .iter()
            .filter_map(|at| listed.get(*at).and_then(Option::as_ref))
            .filter(|path| code.contains(*path) && !is_test_path(path))
            .collect();
        files.sort();
        let weight: f64 = words.iter().map(|&at| ground.looks[at].rarity).sum::<f64>() / files.len().max(1) as f64;
        for file in files {
            found.push((
                file.clone(),
                Evidence { words: words.clone(), via: Via::Commit(title.clone()), link: Link::Suspected, score: weight },
            ));
        }
    }
    found.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then_with(|| a.0.cmp(&b.0)));
    found.truncate(PER_STEP);
    for (file, evidence) in found {
        add((file, None), evidence);
    }
    Ok(())
}

/// Quarto degrau: a marca do glossário volta à declaração que ela aponta.
/// Uma edição confirmada ligou a palavra a ela: a ligação é provada.
fn glossary(ground: &Ground<'_>, add: &mut impl FnMut(Owner, Evidence)) -> Result<()> {
    for (at, docs) in map_glossary::marked(ground.conn, Learned::Decls, &ground.forms())?.into_iter().enumerate() {
        for doc in docs.into_iter().take(PER_STEP) {
            if let Some((path, place)) = located(ground.conn, doc)? {
                add(
                    (path, Some(place)),
                    Evidence { words: vec![at], via: Via::Glossary, link: Link::Proven, score: ground.looks[at].rarity },
                );
            }
        }
    }
    Ok(())
}

/// A evidência do arquivo sem declaração passa às declarações do mesmo
/// arquivo que já têm evidência de alguma das mesmas palavras: função e teste
/// viram uma entrada só. O que nenhuma declaração pede fica na entrada do
/// arquivo.
fn merged(mut found: BTreeMap<Owner, Vec<Evidence>>) -> BTreeMap<Owner, Vec<Evidence>> {
    let files: Vec<Owner> = found.keys().filter(|(_, decl)| decl.is_none()).cloned().collect();
    for owner in files {
        let Some(all) = found.remove(&owner) else { continue };
        let mut left: Vec<Evidence> = Vec::new();
        for evidence in all {
            let targets: Vec<Owner> = found
                .iter()
                .filter(|((path, decl), list)| {
                    *path == owner.0
                        && decl.is_some()
                        && list.iter().any(|other| other.words.iter().any(|word| evidence.words.contains(word)))
                })
                .map(|(key, _)| key.clone())
                .collect();
            if targets.is_empty() {
                left.push(evidence);
                continue;
            }
            for key in targets {
                if let Some(list) = found.get_mut(&key) {
                    list.push(Evidence {
                        words: evidence.words.clone(),
                        via: evidence.via.clone(),
                        link: evidence.link,
                        score: evidence.score,
                    });
                }
            }
        }
        if !left.is_empty() {
            found.insert(owner, left);
        }
    }
    found
}

/// As entradas da busca funda: a nota de cada dono, na ordem da nota, sem o
/// que é muito mais fraco que a primeira, até `limit`. Sem `limit`, todas.
fn entries(found: BTreeMap<Owner, Vec<Evidence>>, missing: &[&Word], limit: Option<usize>) -> Vec<Deeper> {
    let mut out: Vec<Deeper> = found
        .into_iter()
        .map(|((path, decl), list)| {
            let mut words: Vec<usize> = list.iter().flat_map(|evidence| evidence.words.iter().copied()).collect();
            words.sort_unstable();
            words.dedup();
            let mut via: Vec<Via> = Vec::new();
            for evidence in &list {
                if !via.contains(&evidence.via) {
                    via.push(evidence.via.clone());
                }
            }
            let link = list.iter().map(|evidence| evidence.link).min().unwrap_or(Link::Suspected);
            Deeper {
                path,
                decl,
                words: words.into_iter().map(|word| missing[word].plain.clone()).collect(),
                via,
                link,
                score: list.iter().map(|evidence| evidence.score).sum(),
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.link.cmp(&b.link))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.decl.cmp(&b.decl))
    });
    let Some(limit) = limit else { return out };
    let first = out.first().map_or(0.0, |entry| entry.score);
    out.retain(|entry| triage::keeps(entry.score, first));
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::project_map as store;
    use serde_json::{json, Value};
    use std::time::Duration;
    use tempfile::{tempdir, TempDir};

    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    /// Um projeto com o mapa `map`, em JSON, gravado pela porta como o scan
    /// grava: o índice de busca entra junto.
    fn saved(map: &Value) -> TempDir {
        let dir = tempdir().unwrap();
        store::save_at(&model_path(dir.path()), map, "scan 1", &languages()).unwrap();
        dir
    }

    /// A resposta triada da pergunta, como o comando a pede.
    fn ask(dir: &TempDir, query: &str) -> Triaged {
        triage(dir.path(), (query, ""), &languages(), TOP).unwrap()
    }

    /// A busca funda inteira da pergunta, sem o grau que a barra e sem o
    /// corte do muito mais fraco.
    fn whole(dir: &TempDir, query: &str) -> Triaged {
        let db = indexed(&model_path(dir.path()), &languages(), &SEARCHED).unwrap();
        triaged(db.conn(), None, (query, ""), &languages(), TOP, true).unwrap()
    }

    /// A busca funda da pergunta como a resposta a leva, com o corte do muito
    /// mais fraco e o limite, sem o grau que a barra: a busca dos arquivos lê
    /// os comentários, e a fixture de um comentário só já sobe o grau.
    fn deeper_cut(dir: &TempDir, query: &str) -> Vec<Deeper> {
        let db = indexed(&model_path(dir.path()), &languages(), &SEARCHED).unwrap();
        let mut normalizer = Normalizer::new(&languages());
        let words = question(db.conn(), &mut normalizer, query).unwrap();
        let missing: Vec<&Word> = words.iter().collect();
        search_deeper(db.conn(), &mut normalizer, &missing, Some(TOP)).unwrap()
    }

    /// A entrada da busca funda deste arquivo e desta declaração.
    fn entry<'a>(got: &'a Triaged, path: &str, name: Option<&str>) -> Option<&'a Deeper> {
        got.deeper.iter().find(|entry| entry.path == path && entry.decl.as_ref().map(|decl| decl.name.as_str()) == name)
    }

    /// Uma função de dez linhas, com o comentário escrito no corpo.
    fn function(name: &str, line: u64, body_comment: &str) -> Value {
        json!({"kind": "function", "name": name, "line": line, "end_line": line + 9,
               "signature": format!("fn {name}()"), "body_comment": body_comment})
    }

    #[test]
    fn a_lone_strong_finding_is_grade_five_and_the_deep_search_stops_there() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "declarations": [
                function("charge", 1, ""), function("reissue", 12, "reemite o boleto vencido")]}
        ]}));
        let got = ask(&dir, "charge boleto");
        assert_eq!(got.grade, 5, "{got:?}");
        assert_eq!(got.signals.strong, 1);
        assert!(got.deeper.is_empty(), "grade 5 answers with the strong fields and stops: {:?}", got.deeper);
        let deep = whole(&dir, "charge boleto");
        assert!(entry(&deep, "src/pay/gateway.rs", Some("reissue")).is_some(), "the comment was there to find: {deep:?}");
    }

    /// A marca da resposta triada não lê as palavras que faltam: nota 5 com o
    /// primeiro cobrindo as palavras raras e o segundo não é cravada com três
    /// palavras fora dos campos fortes; a mesma resposta com nota 4, ou com o
    /// segundo cobrindo também, é parcial.
    #[test]
    fn a_grade_five_answer_is_pinned_with_three_words_missing() {
        let lone = Signals { words: 1, strong: 1, first: Some(9.0), second: None };
        let words: Vec<String> = ["um", "dois", "tres", "quatro"].map(String::from).to_vec();
        let answer = |grade: u8, lead: Lead| Triaged {
            grade,
            signals: lone,
            words: words.clone(),
            missing: words[1..].to_vec(),
            files: Vec::new(),
            deeper: Vec::new(),
            lead,
        };
        let ahead = Lead { first: 1.0, second: 0.0 };
        assert_eq!(answer(5, ahead).mark(), triage::Mark::Pinned);
        assert_eq!(answer(4, ahead).mark(), triage::Mark::Partial);
        assert_eq!(answer(5, Lead { first: 1.0, second: 1.0 }).mark(), triage::Mark::Partial);
        assert_eq!(answer(0, ahead).mark(), triage::Mark::NotFound);
    }

    /// Um arquivo que só a lista das declarações acha, por uma palavra da
    /// assinatura, é achado: a resposta o traz e o grau não é o de nada
    /// achado, embora o banco dos arquivos não o veja.
    #[test]
    fn a_file_only_the_list_finds_is_answered_and_is_not_reported_as_nothing_found() {
        let dir = saved(&json!({"modules": [
            {"path": "src/relogio.rs", "declarations": [
                {"kind": "function", "name": "agora", "line": 1, "end_line": 3, "signature": "pub fn agora() -> Timestamp"}]},
            {"path": "src/pedido.rs", "declarations": [function("gravar", 1, "")]}
        ]}));
        let got = ask(&dir, "timestamp");
        assert_eq!(got.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["src/relogio.rs"], "{got:?}");
        assert_eq!(got.grade, 1, "{got:?}");
        assert_ne!(got.mark(), triage::Mark::NotFound);
    }

    /// A chance do grau fala do primeiro arquivo do banco: o banco só põe
    /// `timestamp.rs` na frente, com a nota mais alta, mas a ordem única
    /// empata com o arquivo da lista e fica com ele; outro primeiro arquivo
    /// nunca é cravado.
    #[test]
    fn an_answer_whose_first_file_is_not_the_banks_first_is_never_pinned() {
        let dir = saved(&json!({"modules": [
            {"path": "src/relogio.rs", "declarations": [
                {"kind": "function", "name": "agora", "line": 1, "end_line": 3, "signature": "pub fn agora() -> Timestamp"}]},
            {"path": "src/timestamp.rs", "declarations": []}
        ]}));
        let got = ask(&dir, "timestamp");
        assert_eq!(got.files.first().map(|file| file.path.as_str()), Some("src/relogio.rs"), "{got:?}");
        assert_eq!(triage::grade(&got.signals), 5, "the bank alone would give its first file the top grade: {:?}", got.signals);
        assert_eq!((got.grade, got.mark()), (triage::UNSURE_GRADE, triage::Mark::Partial), "{got:?}");
    }

    /// Com o primeiro arquivo da ordem única o mesmo do banco, a resposta
    /// segue cravada.
    #[test]
    fn an_answer_whose_first_file_is_the_banks_first_stays_pinned() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "declarations": [function("charge", 1, "")]}
        ]}));
        let got = ask(&dir, "charge");
        assert_eq!(got.files.first().map(|file| file.path.as_str()), Some("src/pay/gateway.rs"), "{got:?}");
        assert_eq!((got.grade, got.mark()), (5, triage::Mark::Pinned), "{got:?}");
    }

    /// Módulos que não dizem nada da pergunta: fazem as palavras dela serem
    /// raras no índice.
    fn fillers() -> Vec<Value> {
        (0..12)
            .map(|n| {
                json!({"path": format!("src/outro{n}.rs"), "declarations": [
                    {"kind": "function", "name": format!("fazer{n}"), "line": 1, "end_line": 5,
                     "signature": format!("fn fazer{n}()"), "doc": "algo bem diferente aqui"}]})
            })
            .collect()
    }

    fn documented(path: &str, name: &str, doc: &str) -> Value {
        json!({"path": path, "declarations": [
            {"kind": "function", "name": name, "line": 1, "end_line": 5, "signature": format!("fn {name}()"), "doc": doc}]})
    }

    /// A conferência dos primeiros candidatos vale na resposta: dos dois
    /// arquivos que o índice põe lado a lado pela `fatura`, fica na frente o
    /// que traz no corpo, lido do disco, a palavra `vencida` que o índice
    /// não tem nele, seja qual for a ordem de antes.
    #[test]
    fn the_answer_puts_first_the_file_whose_body_on_disk_carries_the_word_the_index_lacks() {
        for winner in ["src/a.rs", "src/b.rs"] {
            let mut modules = fillers();
            modules.push(documented("src/a.rs", "emitir", "emite a fatura"));
            modules.push(documented("src/b.rs", "cobrar", "cobra a fatura"));
            modules.push(documented("src/x.rs", "listar", "lista a cobranca vencida"));
            let dir = saved(&json!({ "modules": modules }));
            std::fs::create_dir_all(dir.path().join("src")).unwrap();
            std::fs::write(dir.path().join(winner), "fn corpo() {\n    let motivo = \"vencida\";\n}\n").unwrap();
            let got = ask(&dir, "fatura vencida");
            assert_eq!(got.files.first().map(|file| file.path.as_str()), Some(winner), "{got:?}");
        }
    }

    /// Nota 5 crava só com a frente: o segundo candidato que também cobre a
    /// palavra da pergunta deixa a resposta parcial, e o que só o primeiro
    /// cobre a deixa cravada.
    #[test]
    fn a_grade_five_answer_is_pinned_only_when_the_second_candidate_does_not_cover() {
        let mut alone = fillers();
        alone.push(json!({"path": "src/pay/gateway.rs", "declarations": [function("charge", 1, "")]}));
        let dir = saved(&json!({ "modules": alone }));
        let got = ask(&dir, "charge");
        assert_eq!((got.grade, got.mark()), (5, triage::Mark::Pinned), "{got:?}");
        assert!(got.lead.leads(), "{:?}", got.lead);

        let mut shared = fillers();
        shared.push(json!({"path": "src/pay/gateway.rs", "declarations": [function("charge", 1, "")]}));
        shared.push(documented("src/pay/card.rs", "cobrar", "cobra no cartao, no charge do banco"));
        let dir = saved(&json!({ "modules": shared }));
        let got = ask(&dir, "charge");
        assert!(!got.lead.leads(), "the second file covers the word too: {:?}", got.lead);
        assert_eq!(got.mark(), triage::Mark::Partial, "{got:?}");
    }

    #[test]
    fn a_question_nothing_answers_is_grade_zero_with_its_words_split() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "declarations": [function("charge", 1, "")]}
        ]}));
        let got = ask(&dir, "o quebra-cabeca do zzyzx");
        assert_eq!((got.grade, got.files.len(), got.deeper.len()), (0, 0, 0), "{got:?}");
        assert_eq!(got.words, ["quebra", "cabeca", "zzyzx"]);
    }

    #[test]
    fn a_word_only_a_comment_holds_finds_the_file_and_comes_back_to_the_function() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "declarations": [
                function("charge", 1, ""), function("reissue", 12, "reemite o boleto vencido")]}
        ]}));
        let answer = ask(&dir, "boleto vencido");
        assert_eq!((answer.signals.strong, answer.files.len()), (0, 1), "the comment brings the file, in no strong field: {answer:?}");
        let got = whole(&dir, "boleto vencido");
        assert_eq!(got.deeper.len(), 1, "{:?}", got.deeper);
        let found = &got.deeper[0];
        assert_eq!((found.path.as_str(), found.decl.as_ref().map(|decl| decl.name.as_str())), ("src/pay/gateway.rs", Some("reissue")));
        assert_eq!((found.decl.as_ref().map(|decl| (decl.line, decl.end_line)), &found.via[..]), (Some((12, 21)), &[Via::Comment][..]));
        assert_eq!((found.link, found.words.as_slice()), (Link::Proven, &["boleto".to_string(), "vencido".to_string()][..]));
    }

    #[test]
    fn a_comment_goes_back_to_the_innermost_function_that_holds_it() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "declarations": [
                {"kind": "function", "name": "outer", "line": 1, "end_line": 30, "body_comment": "reemite o boleto vencido"},
                {"kind": "function", "name": "inner", "line": 10, "end_line": 15, "body_comment": "reemite o boleto vencido"},
                function("apart", 40, "")]}
        ]}));
        let got = whole(&dir, "boleto vencido");
        let names: Vec<_> = got.deeper.iter().map(|entry| entry.decl.as_ref().map(|decl| decl.name.clone())).collect();
        assert_eq!(names, [Some("inner".to_string())], "{:?}", got.deeper);
    }

    #[test]
    fn a_function_that_documents_the_word_itself_stays_beside_the_inner_one() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "declarations": [
                {"kind": "function", "name": "outer", "line": 1, "end_line": 30, "doc": "Reemite o boleto vencido."},
                {"kind": "function", "name": "inner", "line": 10, "end_line": 15, "body_comment": "reemite o boleto vencido"}]}
        ]}));
        let got = whole(&dir, "boleto vencido");
        assert!(entry(&got, "src/pay/gateway.rs", Some("outer")).is_some(), "{:?}", got.deeper);
        assert!(entry(&got, "src/pay/gateway.rs", Some("inner")).is_some(), "{:?}", got.deeper);
    }

    #[test]
    fn a_test_goes_back_to_the_file_it_imports_as_a_proven_link() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "tests": ["tests/pay_flow.rs"], "declarations": [function("charge", 1, "")]},
            {"path": "src/pay/refund.rs", "declarations": [function("refund", 1, "")], "deps": ["src/pay/gateway.rs"]},
            {"path": "tests/pay_flow.rs", "deps": ["src/pay/gateway.rs"], "file_comment": "confere o estorno programado",
             "declarations": [function("it_works", 1, "")]}
        ]}));
        let got = whole(&dir, "estorno programado");
        assert_eq!(got.deeper.len(), 1, "one step only, the importer of the file stays out: {:?}", got.deeper);
        let found = &got.deeper[0];
        assert_eq!((found.path.as_str(), found.decl.is_none(), found.link), ("src/pay/gateway.rs", true, Link::Proven));
        assert_eq!(found.via, [Via::Test("tests/pay_flow.rs".to_string())]);
    }

    #[test]
    fn a_test_that_only_uses_the_files_is_a_suspected_link_split_between_them() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "tests": ["tests/flow.rs"], "declarations": [function("charge", 1, "")]},
            {"path": "src/pay/refund.rs", "tests": ["tests/flow.rs"], "declarations": [function("refund", 1, "")]},
            {"path": "tests/flow.rs", "file_comment": "cobre o cancelamento do carne", "declarations": [function("it_works", 1, "")]}
        ]}));
        let got = whole(&dir, "cancelamento carne");
        let paths: Vec<_> = got.deeper.iter().map(|entry| (entry.path.as_str(), entry.link)).collect();
        assert_eq!(paths, [("src/pay/gateway.rs", Link::Suspected), ("src/pay/refund.rs", Link::Suspected)]);
        let whole_test = whole(&dir, "cancelamento carne");
        let alone = whole_test.deeper.iter().map(|entry| entry.score).fold(f64::MAX, f64::min);
        let named = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "tests": ["tests/flow.rs"], "declarations": [function("charge", 1, "")]},
            {"path": "tests/flow.rs", "file_comment": "cobre o cancelamento do carne", "declarations": [function("it_works", 1, "")]}
        ]}));
        let one = whole(&named, "cancelamento carne");
        assert!(one.deeper[0].score > alone * 1.5, "the suspicion is shared: {} vs {alone}", one.deeper[0].score);
    }

    #[test]
    fn the_test_that_carries_the_file_name_is_a_proven_link_without_the_import() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "tests": ["tests/gateway.rs"], "declarations": [function("charge", 1, "")]},
            {"path": "tests/gateway.rs", "file_comment": "cobre o cancelamento do carne", "declarations": [function("it_works", 1, "")]}
        ]}));
        let got = whole(&dir, "cancelamento carne");
        assert_eq!(got.deeper.iter().map(|entry| entry.link).collect::<Vec<_>>(), [Link::Proven], "{:?}", got.deeper);
    }

    #[test]
    fn a_function_and_the_test_that_speak_of_the_same_word_are_one_entry() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "tests": ["tests/gateway.rs"], "deps": [],
             "declarations": [function("charge", 1, ""), function("reissue", 12, "reemite o boleto vencido")]},
            {"path": "tests/gateway.rs", "deps": ["src/pay/gateway.rs"], "file_comment": "confere o boleto vencido",
             "declarations": [function("it_works", 1, "")]}
        ]}));
        let got = ask(&dir, "boleto vencido");
        assert_eq!(got.deeper.len(), 1, "{:?}", got.deeper);
        let found = &got.deeper[0];
        assert_eq!(found.decl.as_ref().map(|decl| decl.name.as_str()), Some("reissue"));
        assert_eq!(found.via, [Via::Comment, Via::Test("tests/gateway.rs".to_string())]);
        assert_eq!(found.link, Link::Proven);
    }

    #[test]
    fn a_test_of_another_word_stays_its_own_entry_on_the_file() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "tests": ["tests/gateway.rs"], "deps": [],
             "declarations": [function("charge", 1, ""), function("reissue", 12, "reemite o boleto")]},
            {"path": "tests/gateway.rs", "deps": ["src/pay/gateway.rs"], "file_comment": "confere o vencimento",
             "declarations": [function("it_works", 1, "")]}
        ]}));
        let got = whole(&dir, "boleto vencimento");
        assert!(entry(&got, "src/pay/gateway.rs", Some("reissue")).is_some(), "{:?}", got.deeper);
        assert!(entry(&got, "src/pay/gateway.rs", None).is_some(), "{:?}", got.deeper);
    }

    #[test]
    fn each_owner_shows_up_once_with_everything_that_found_it() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "tests": ["tests/gateway.rs", "tests/gateway_slow.rs"], "deps": [],
             "declarations": [function("reissue", 12, "reemite o boleto vencido")]},
            {"path": "tests/gateway.rs", "deps": ["src/pay/gateway.rs"], "file_comment": "confere o boleto vencido",
             "declarations": [function("it_works", 1, "")]},
            {"path": "tests/gateway_slow.rs", "deps": ["src/pay/gateway.rs"], "file_comment": "espera o boleto vencido",
             "declarations": [function("it_waits", 1, "")]}
        ]}));
        let got = whole(&dir, "boleto vencido");
        let owners: Vec<_> = got.deeper.iter().map(|entry| (entry.path.clone(), entry.decl.clone())).collect();
        let unique: std::collections::BTreeSet<_> = owners.iter().collect();
        assert_eq!(owners.len(), unique.len(), "{owners:?}");
        assert_eq!(got.deeper.len(), 1, "{:?}", got.deeper);
        assert_eq!(got.deeper[0].via.len(), 3, "{:?}", got.deeper[0].via);
    }

    #[test]
    fn a_commit_title_goes_back_to_the_files_it_touched_as_a_suspected_link() {
        let dir = saved(&json!({
            "modules": [
                {"path": "src/pay/gateway.rs", "declarations": [function("charge", 1, "")]},
                {"path": "src/pay/refund.rs", "declarations": [function("refund", 1, "")]}],
            "history": {"paths": ["src/pay/gateway.rs", "src/pay/refund.rs"], "commits": [
                {"id": "c1", "at": 100, "title": "Estorno parcial do carne", "changed": [0]},
                {"id": "c2", "at": 200, "title": "Ajusta a fila", "changed": [1]}]}
        }));
        let got = ask(&dir, "estorno parcial");
        assert_eq!(got.deeper.len(), 1, "{:?}", got.deeper);
        let found = &got.deeper[0];
        assert_eq!((found.path.as_str(), found.link), ("src/pay/gateway.rs", Link::Suspected));
        assert_eq!(found.via, [Via::Commit("Estorno parcial do carne".to_string())]);
    }

    #[test]
    fn a_map_without_history_gives_nothing_by_commit() {
        let dir = saved(&json!({"modules": [{"path": "src/pay/gateway.rs", "declarations": [function("charge", 1, "")]}]}));
        assert_eq!(ask(&dir, "estorno parcial").grade, 0);
    }

    #[test]
    fn a_word_an_edit_once_tied_to_a_function_comes_back_to_it_as_a_proven_link() {
        let dir = saved(&json!({"modules": [
            {"path": "src/rounds.rs", "declarations": [{"kind": "function", "name": "collect_leftover", "line": 1, "end_line": 5}]},
            {"path": "src/cash.rs", "declarations": [function("close_month", 1, "")]}
        ]}));
        let model = model_path(dir.path());
        map_glossary::record_search(&model, "s1", "sobra", &languages(), &[]).unwrap();
        let wait = Duration::from_secs(1);
        let marks = map_glossary::confirm_edit(&model, "s1", "src/rounds.rs", &[(2, 2)], &languages(), wait).unwrap();
        assert_eq!(marks.len(), 1, "{marks:?}");
        let got = whole(&dir, "sobra");
        assert_eq!(got.deeper.len(), 1, "{:?}", got.deeper);
        assert_eq!(got.deeper[0].decl.as_ref().map(|decl| decl.name.as_str()), Some("collect_leftover"));
        assert_eq!((got.deeper[0].via.as_slice(), got.deeper[0].link), (&[Via::Glossary][..], Link::Proven));
    }

    #[test]
    fn a_finding_much_weaker_than_the_first_stays_out_of_the_answer() {
        let dir = saved(&json!({"modules": [
            {"path": "src/pay/gateway.rs", "declarations": [
                function("reissue", 1, "reemite o boleto vencido do carne"),
                function("remind", 12, "avisa do carne")]}
        ]}));
        let deep = whole(&dir, "boleto vencido carne");
        let (first, weak) = (entry(&deep, "src/pay/gateway.rs", Some("reissue")), entry(&deep, "src/pay/gateway.rs", Some("remind")));
        let (first, weak) = (first.expect("first").score, weak.expect("weak").score);
        assert!(!triage::keeps(weak, first), "the fixture needs a much weaker finding: {weak} against {first}");
        let cut = deeper_cut(&dir, "boleto vencido carne");
        let names: Vec<_> = cut.iter().filter_map(|entry| entry.decl.as_ref().map(|decl| decl.name.as_str())).collect();
        assert_eq!(names, ["reissue"]);
    }

    #[test]
    fn the_answer_keeps_the_five_best_entries_and_the_whole_search_all_of_them() {
        let functions: Vec<Value> = (0..7).map(|at| function(&format!("resend_{at}"), 1 + 10 * at, "reemite o boleto vencido")).collect();
        let dir = saved(&json!({"modules": [{"path": "src/pay/gateway.rs", "declarations": functions}]}));
        assert_eq!(deeper_cut(&dir, "boleto vencido").len(), TOP);
        assert_eq!(whole(&dir, "boleto vencido").deeper.len(), 7);
    }

    /// A medida da triagem numa régua de buscas, um arquivo JSON com as
    /// buscas (`MAP_TRIAGE_RULER`): grava em `MAP_TRIAGE_OUT` uma linha por
    /// busca, com os sinais, a posição do primeiro arquivo certo entre os
    /// achados e a busca funda inteira, com a nota de cada entrada e se ela
    /// acerta. Imprime, por projeto, em quantas buscas o primeiro achado é o
    /// certo e em quantas o certo está entre os cinco, e quantas a marca da
    /// resposta crava (o grau 5 com a frente da conferência), com o primeiro
    /// certo e a régua reprovando; por grau, os mesmos números; por corte da
    /// chance nas buscas de grau 5, para comparar com a marca, com e sem a
    /// exigência de que nenhuma palavra falte nos campos fortes, quantas
    /// ficam cravadas; e, por corte da busca funda, quantas sobras a resposta
    /// leva e quantas buscas o corte resgata.
    #[test]
    #[ignore = "mede com os mapas dos projetos de prova"]
    fn measure_the_ruler() {
        let Ok(ruler) = std::env::var("MAP_TRIAGE_RULER") else { panic!("MAP_TRIAGE_RULER points to the ruler file") };
        let out = std::env::var("MAP_TRIAGE_OUT").expect("MAP_TRIAGE_OUT points to the file to write");
        let ruler: Value = serde_json::from_str(&std::fs::read_to_string(ruler).unwrap()).unwrap();
        let languages = Languages::new(["pt-BR", "en-US"]);
        crate::io::map_search::tests::weights_from_env();
        let off = std::env::var("MAP_RANKS_SENSE_OFF").unwrap_or_default();
        crate::io::map_sense::tuning::switch_off(off.contains("near"), off.contains("meaning"));
        let phrase = std::env::var("MAP_TRIAGE_PHRASE").is_ok();
        let mut lines: Vec<String> = Vec::new();
        // Por grau: as buscas, as de primeiro achado certo e as com o certo
        // entre os cinco. Das buscas de grau baixo com busca funda: a posição
        // do certo entre os achados e as notas com o acerto de cada entrada.
        let mut grades: BTreeMap<u8, [usize; 3]> = BTreeMap::new();
        let mut deep: Vec<(usize, Vec<(f64, bool)>)> = Vec::new();
        // Por busca de grau 5: a chance, quantas palavras faltam nos campos
        // fortes e a posição do primeiro arquivo certo (0: fora da lista).
        let mut top: Vec<(f64, usize, usize)> = Vec::new();
        // Por projeto: as buscas, as de primeiro achado certo e as com o certo
        // entre os cinco; e as cravadas pela marca da resposta, as de primeiro
        // certo e as que a régua reprova.
        let mut projects: BTreeMap<String, [usize; 3]> = BTreeMap::new();
        let mut pinned_by_mark: BTreeMap<String, [usize; 3]> = BTreeMap::new();
        for search in ruler["searches"].as_array().unwrap() {
            let text = |key: &str| search[key].as_str().unwrap().to_string();
            let db = indexed(Path::new(&text("model")), &languages, &SEARCHED).unwrap();
            let started = std::time::Instant::now();
            let asked = if phrase { text("intent") } else { text("query") };
            let root: Option<String> = db.conn().query_row("SELECT root FROM census", [], |row| row.get(0)).ok();
            let got = triaged(db.conn(), root.as_deref().map(Path::new), (&asked, &text("intent")), &languages, TOP, true).unwrap();
            let millis = started.elapsed().as_secs_f64() * 1000.0;
            let targets: Vec<(String, String)> = search["targets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| (t[0].as_str().unwrap().to_string(), t[1].as_str().unwrap().to_string()))
                .collect();
            let right = |path: &str| targets.iter().any(|(target, _)| target == path);
            let rank = got.files.iter().position(|file| right(&file.path)).map_or(0, |at| at + 1);
            let seen = grades.entry(got.grade).or_default();
            *seen = [seen[0] + 1, seen[1] + usize::from(rank == 1), seen[2] + usize::from(rank >= 1)];
            let project = text("key").split('|').next().unwrap_or_default().to_string();
            let seen = projects.entry(project.clone()).or_default();
            *seen = [seen[0] + 1, seen[1] + usize::from(rank == 1), seen[2] + usize::from((1..=TOP).contains(&rank))];
            if got.mark() == triage::Mark::Pinned {
                let seen = pinned_by_mark.entry(project).or_default();
                *seen = [seen[0] + 1, seen[1] + usize::from(rank == 1), seen[2] + usize::from(rank == 0)];
            }
            if got.grade >= 5 {
                top.push((triage::chance(&got.signals), got.missing.len(), rank));
            }
            if got.grade <= triage::DEEP_UNTIL && !got.deeper.is_empty() {
                deep.push((rank, got.deeper.iter().map(|entry| (entry.score, right(&entry.path))).collect()));
            }
            let deeper: Vec<Value> = got
                .deeper
                .iter()
                .take(40)
                .map(|entry| {
                    let function = entry.decl.as_ref().is_some_and(|decl| {
                        targets.iter().any(|(target, name)| *target == entry.path && *name == decl.name)
                    });
                    json!({
                        "score": entry.score, "file": right(&entry.path), "function": function,
                        "proven": entry.link == Link::Proven, "decl": entry.decl.is_some(),
                        "via": entry.via.iter().map(|via| match via {
                            Via::Comment => "comment", Via::Test(_) => "test", Via::Commit(_) => "commit", Via::Glossary => "glossary"
                        }).collect::<Vec<_>>(),
                    })
                })
                .collect();
            lines.push(
                json!({
                    "key": text("key"), "words": got.signals.words, "strong": got.signals.strong,
                    "first": got.signals.first, "second": got.signals.second, "found": got.files.len(),
                    "chance": triage::chance(&got.signals), "missing": got.missing.len(),
                    "grade": got.grade, "lead": [got.lead.first, got.lead.second],
                    "rank": rank, "millis": millis, "deeper": deeper,
                })
                .to_string(),
            );
        }
        std::fs::write(out, lines.join("\n")).unwrap();
        for (project, [all, first, five]) in &projects {
            eprintln!("project {project}: {all} searches, first right {first}, right among five {five}");
        }
        for (project, [all, first, wrong]) in &pinned_by_mark {
            eprintln!("pinned {project}: {all} searches, first right {first}, ruler rejects {wrong}");
        }
        for (grade, [all, first, five]) in grades.iter().rev() {
            eprintln!("grade {grade}: {all} searches, first right {first}, right among five {five}");
        }
        for (cut, whole_question) in
            [0.8, 0.85, 0.9, 0.93, 0.95, 0.97, 0.98, 0.99].into_iter().flat_map(|cut| [(cut, false), (cut, true)])
        {
            let pinned: Vec<&(f64, usize, usize)> =
                top.iter().filter(|(chance, missing, _)| *chance >= cut && (!whole_question || *missing == 0)).collect();
            let first = pinned.iter().filter(|(_, _, rank)| *rank == 1).count();
            let wrong = pinned.iter().filter(|(_, _, rank)| *rank == 0).count();
            eprintln!(
                "pinned from chance {cut}{}: {} searches, first right {first}, ruler rejects {wrong}",
                if whole_question { " with no word missing" } else { "" },
                pinned.len()
            );
        }
        for ratio in [0.0, 0.5, 0.6, 0.7, 0.75, 0.8, 0.9, 1.0] {
            let (mut kept, mut leftovers, mut rescued) = (0, 0, 0);
            for (rank, entries) in &deep {
                let top = entries[0].0;
                let shown: Vec<bool> =
                    entries.iter().filter(|(score, _)| *score >= ratio * top).take(TOP).map(|(_, right)| *right).collect();
                kept += shown.len();
                leftovers += shown.iter().filter(|right| !**right).count();
                rescued += usize::from(*rank == 0 && shown.contains(&true));
            }
            let rows = deep.len().max(1) as f64;
            eprintln!(
                "ratio {ratio}: {:.2} entries and {:.2} leftovers per answer, {rescued} searches rescued",
                kept as f64 / rows,
                leftovers as f64 / rows
            );
        }
    }
}
