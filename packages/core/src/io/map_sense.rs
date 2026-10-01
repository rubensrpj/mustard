//! `map_sense` — o sentido das palavras e das declarações na busca do mapa.
//!
//! O scan grava, no bloco `meaning` ([`crate::io::map_meaning`]), um vetor por
//! declaração e um por palavra do projeto. A busca lê esses vetores de duas
//! maneiras, e nenhuma vale sem eles: o mapa sem vetores responde como sempre.
//!
//! - **As formas vizinhas de cada palavra** ([`Near`]). Cada palavra da
//!   pergunta que não é de ligação ganha até [`SYNONYMS`] palavras do projeto
//!   com cosseno de pelo menos [`SYNONYM_COSINE`] — `apagar` acha o projeto
//!   que só escreve `remove` — e, em projeto de duas línguas, até
//!   [`OTHER_LANGUAGE`] palavras da outra língua do projeto, com o limite de
//!   cosseno próprio dessa travessia ([`OTHER_LANGUAGE_COSINE`]) — `parcela`
//!   acha `splitInstallments`. As formas dessas palavras entram na nota com
//!   metade do peso das que a pergunta escreveu
//!   ([`crate::domain::search::NEAR_FORM_WEIGHT`]).
//! - **A ordem dos vetores** ([`Meaning`]). O vetor do pedido contra o de
//!   todas as declarações: as declarações da mais perto para a mais longe e
//!   os arquivos, cada um pela melhor declaração dele. Ela entra na lista de
//!   até cem candidatos do filtro, somada por posição recíproca à ordem única
//!   das palavras com metade do peso
//!   ([`crate::domain::search::VECTOR_WEIGHT`]); a resposta do banco e a
//!   marca de "cravado" continuam lendo só as palavras.
//!
//! Onde a marca de "cravado" e a resposta já são certas pelas palavras
//! escritas, o sentido não muda nada: ele só entra onde a busca de sempre
//! não achou, ou não tinha certeza.
//!
//! Função de palavra de ligação nunca ganha vizinha: `de`, `the` e `para`
//! ficam fora da conta como sempre.
//!
//! **A palavra escrita não perde para a vizinha nem para o vetor.** A palavra
//! que o mapa escreve, em qualquer campo que conta na nota, dos arquivos ou
//! das declarações, não ganha forma vizinha: as vizinhas são o socorro da
//! palavra que o mapa não acha. Na régua de nomes da Sialia, deixar as
//! vizinhas de toda palavra entrarem na nota tirou o primeiro lugar de 6
//! buscas (de 57 para 51): a soma de muitas vizinhas de palavras que o mapa
//! escreve passava um nome escrito por inteiro. Os vetores entram só na lista
//! de candidatos depois da cabeça, que segue a ordem das palavras. Como a
//! palavra escrita não precisa de vizinha, ela também não vai ao modelo: o
//! modelo e as palavras do projeto só são lidos quando alguma palavra do
//! pedido não está no mapa.
//!
//! **A língua da palavra.** O mapa não guarda a língua de cada palavra; guarda
//! onde ela aparece. A palavra é do código quando o índice a tem no nome, no
//! caminho ou na assinatura de alguma declaração, e é do texto quando o tem na
//! documentação, nas mensagens ou nos comentários. A palavra perguntada que o
//! código não escreve procura as vizinhas entre as do código; a que o código
//! escreve e o texto não, entre as do texto; a que os dois escrevem já está
//! nas duas línguas e não atravessa.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, Statement};

use crate::domain::normalize::{plain_words, Languages, Normalizer};
use crate::io::map_meaning::{quantized_vector, ranked_declarations, Neighbor, ProjectWords};
use crate::io::map_index::{as_indexed, is_read};
use crate::platform::error::Result;

/// Quantas palavras do projeto perto de uma palavra da pergunta viram formas
/// vizinhas dela.
pub(super) const SYNONYMS: usize = 3;

/// O cosseno mínimo de uma palavra do projeto para valer como sinônimo.
pub(super) const SYNONYM_COSINE: f32 = 0.6;

/// Quantas palavras da outra língua do projeto viram formas vizinhas de uma
/// palavra da pergunta.
pub(super) const OTHER_LANGUAGE: usize = 3;

/// O cosseno mínimo de uma palavra da outra língua do projeto. Palavras de
/// línguas diferentes ficam mais longe entre si que as da mesma língua, e por
/// isso o limite é próprio. Saiu da régua de buscas em português cujo arquivo
/// certo tem nomes em inglês: nos 177 pedidos dos assuntos pares (metade A),
/// dos limites de 0,20 a 0,60 o de 0,35 soma mais acertos na ordem das
/// palavras, na resposta e na lista das declarações (o certo em primeiro e
/// entre os cinco primeiros), e nos 177 dos ímpares (metade B) ele fica 25
/// acertos à frente do 0,60 de partida. Entre 0,30 e 0,50 a diferença é de
/// poucos acertos; abaixo de 0,30 a metade A perde.
pub(super) const OTHER_LANGUAGE_COSINE: f32 = 0.35;

/// A tabela de palavras do nível das declarações do índice.
const DECL_VOCAB: &str = "decl_vocab";

/// As colunas onde uma palavra é do código: o nome, o caminho e a assinatura.
const CODE_COLUMNS: [&str; 3] = ["name", "path", "signature"];

/// As colunas onde uma palavra é do texto: a documentação, as mensagens e os
/// comentários.
const TEXT_COLUMNS: [&str; 6] = ["doc", "log", "error", "text", "whole_doc", "body_comment"];

/// As palavras vizinhas de uma palavra da pergunta: as formas dela mesma e as
/// das palavras do projeto que valem como sinônimo ou como a mesma palavra na
/// outra língua.
#[derive(Debug, Clone)]
struct Asked {
    /// As formas da palavra perguntada, nas línguas do projeto.
    own: Vec<String>,
    /// As formas das palavras vizinhas, sem repetir as dela.
    near: Vec<String>,
}

/// As formas vizinhas de cada palavra de um texto.
#[derive(Debug, Clone, Default)]
pub(super) struct Near {
    asked: Vec<Asked>,
}

impl Near {
    /// Nenhuma forma vizinha: a busca por palavras de sempre.
    pub(super) fn none() -> Self {
        Self::default()
    }

    /// As formas vizinhas das palavras de `text` no mapa aberto em `conn`.
    /// Mapa sem vetores, ou texto sem palavra que o modelo conheça, dá
    /// [`Near::none`].
    pub(super) fn of(conn: &Connection, languages: &Languages, text: &str) -> Result<Self> {
        let crossing = languages.codes().len() > 1;
        let floor = if crossing { SYNONYM_COSINE.min(OTHER_LANGUAGE_COSINE) } else { SYNONYM_COSINE };
        let mut normalizer = Normalizer::new(languages);
        let mut pools = Pools::prepare(conn)?;
        let mut seen: HashSet<String> = HashSet::new();
        let mut asked: Vec<Asked> = Vec::new();
        // As palavras do projeto e o modelo só são lidos quando alguma palavra
        // da pergunta precisa de vizinha.
        let mut project: Option<ProjectWords> = None;
        for word in plain_words(text) {
            if normalizer.is_function_word(&word) || !seen.insert(word.clone()) {
                continue;
            }
            let own = normalizer.word_forms(&word);
            if is_read(conn, &own)? {
                continue;
            }
            if project.is_none() {
                project = Some(ProjectWords::read(conn)?);
            }
            let project = project.as_ref().expect("the words of the project were just read");
            if project.is_empty() {
                return Ok(Self::none());
            }
            #[cfg(test)]
            tuning::count_encoding();
            let Some(vector) = quantized_vector(&word) else { continue };
            let neighbors = project.near(&word, &vector, floor);
            if neighbors.is_empty() {
                continue;
            }
            let other = if crossing { pools.other_of(&own)? } else { None };
            let near = choose(&neighbors, &own, other.map(|pool| (pool, &mut pools)))?;
            if !near.is_empty() {
                asked.push(Asked { own, near });
            }
        }
        Ok(Self { asked })
    }

    /// Se nenhuma palavra da pergunta ganhou forma vizinha.
    pub(super) fn is_empty(&self) -> bool {
        self.asked.is_empty()
    }

    /// Para cada palavra de `words`, as formas vizinhas das palavras da
    /// pergunta que ela traz: a palavra traz uma palavra da pergunta quando
    /// divide com ela alguma forma. Lista vazia onde a palavra não tem
    /// vizinha, e sem repetir forma que a palavra já tem.
    pub(super) fn aligned(&self, words: &[Vec<String>]) -> Vec<Vec<String>> {
        words
            .iter()
            .map(|word| {
                let mut out: Vec<String> = Vec::new();
                for asked in self.asked.iter().filter(|asked| asked.own.iter().any(|form| word.contains(form))) {
                    for form in &asked.near {
                        if !word.contains(form) && !out.contains(form) {
                            out.push(form.clone());
                        }
                    }
                }
                out
            })
            .collect()
    }
}

/// A contagem das palavras que a busca dá ao modelo, só nos testes.
#[cfg(test)]
pub(super) mod tuning {
    thread_local! {
        static ENCODED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// Conta uma palavra que a busca deu ao modelo, nesta linha de execução.
    pub(super) fn count_encoding() {
        ENCODED.with(|count| count.set(count.get() + 1));
    }

    /// Quantas palavras a busca deu ao modelo, nesta linha de execução, até
    /// aqui.
    pub(super) fn encoded() -> usize {
        ENCODED.with(std::cell::Cell::get)
    }
}

/// De onde uma palavra vem no índice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Pool {
    /// Nome, caminho ou assinatura de alguma declaração.
    Code,
    /// Documentação, mensagem ou comentário de alguma declaração.
    Text,
}

/// As consultas "esta palavra aparece nesta origem?" ao índice, com a
/// resposta de cada uma guardada.
struct Pools<'c> {
    conn: &'c Connection,
    code: Statement<'c>,
    text: Statement<'c>,
    known: HashMap<(String, Pool), bool>,
}

impl<'c> Pools<'c> {
    fn prepare(conn: &'c Connection) -> Result<Self> {
        let ask = |columns: &[&str]| {
            let list = columns.iter().map(|column| format!("'{column}'")).collect::<Vec<_>>().join(", ");
            conn.prepare(&format!("SELECT 1 FROM {DECL_VOCAB} WHERE term = ?1 AND col IN ({list}) LIMIT 1"))
        };
        Ok(Self { conn, code: ask(&CODE_COLUMNS)?, text: ask(&TEXT_COLUMNS)?, known: HashMap::new() })
    }

    /// Se alguma das `forms`, como o índice as grava, aparece na origem.
    fn holds(&mut self, forms: &[String], pool: Pool) -> Result<bool> {
        let key = (forms.join(" "), pool);
        if let Some(&known) = self.known.get(&key) {
            return Ok(known);
        }
        let indexed = as_indexed(self.conn, &[forms.to_vec()])?;
        let statement = match pool {
            Pool::Code => &mut self.code,
            Pool::Text => &mut self.text,
        };
        let mut found = false;
        for form in indexed.iter().flatten() {
            if statement.exists([form])? {
                found = true;
                break;
            }
        }
        self.known.insert(key, found);
        Ok(found)
    }

    /// A origem das palavras da outra língua para a palavra de formas `own`:
    /// o código quando ele não a escreve, o texto quando só o código a
    /// escreve, e nenhuma quando os dois a escrevem.
    fn other_of(&mut self, own: &[String]) -> Result<Option<Pool>> {
        if !self.holds(own, Pool::Code)? {
            return Ok(Some(Pool::Code));
        }
        if !self.holds(own, Pool::Text)? {
            return Ok(Some(Pool::Text));
        }
        Ok(None)
    }
}

/// As formas vizinhas de uma palavra de formas `own`, entre as palavras do
/// projeto `neighbors` (da mais perto para a mais longe, com o cosseno
/// mínimo das duas travessias): até [`SYNONYMS`] sinônimos de qualquer
/// origem e, com a origem da outra língua, até [`OTHER_LANGUAGE`] dela. A
/// palavra cujas formas a pergunta ou uma escolhida antes já tem não conta:
/// ela não traz nada de novo.
fn choose(
    neighbors: &[Neighbor],
    own: &[String],
    other: Option<(Pool, &mut Pools<'_>)>,
) -> Result<Vec<String>> {
    let mut taken: HashSet<String> = own.iter().cloned().collect();
    let mut out: Vec<String> = Vec::new();
    let mut take = |forms: &[String], taken: &mut HashSet<String>| -> bool {
        if forms.is_empty() || forms.iter().all(|form| taken.contains(form)) {
            return false;
        }
        for form in forms {
            if taken.insert(form.clone()) {
                out.push(form.clone());
            }
        }
        true
    };
    let split = |neighbor: &Neighbor| neighbor.forms.split_whitespace().map(str::to_string).collect::<Vec<_>>();
    let mut synonyms = 0;
    for neighbor in neighbors.iter().filter(|neighbor| neighbor.cosine >= SYNONYM_COSINE) {
        if synonyms == SYNONYMS {
            break;
        }
        if take(&split(neighbor), &mut taken) {
            synonyms += 1;
        }
    }
    if let Some((pool, pools)) = other {
        let limit = OTHER_LANGUAGE_COSINE;
        let mut crossed = 0;
        for neighbor in neighbors.iter().filter(|neighbor| neighbor.cosine >= limit) {
            if crossed == OTHER_LANGUAGE {
                break;
            }
            let forms = split(neighbor);
            if pools.holds(&forms, pool)? && take(&forms, &mut taken) {
                crossed += 1;
            }
        }
    }
    Ok(out)
}

/// O que a ordem única lê do sentido, além das palavras que a pergunta
/// escreve.
#[derive(Debug, Clone, Default)]
pub(super) struct Sense {
    /// As formas vizinhas das palavras da pergunta.
    pub near: Near,
    /// Se a lista de candidatos soma a ordem dos vetores.
    pub meaning: bool,
}

impl Sense {
    /// Só as palavras escritas: a busca de sempre, também no mapa com vetores.
    pub(super) fn off() -> Self {
        Self::default()
    }

    /// O sentido do pedido `query` e `intent` no mapa aberto: as formas
    /// vizinhas e, com `meaning`, a ordem dos vetores na lista de candidatos.
    /// Mapa sem vetores dá [`Sense::off`].
    pub(super) fn read(
        conn: &Connection,
        languages: &Languages,
        (query, intent): (&str, &str),
        meaning: bool,
    ) -> Result<Self> {
        let request = format!("{query} {intent}");
        Ok(Self { near: Near::of(conn, languages, request.trim())?, meaning })
    }

    /// Se a ordem das palavras muda com o sentido: alguma palavra ganhou
    /// forma vizinha.
    pub(super) fn changes_words(&self) -> bool {
        !self.near.is_empty()
    }
}

/// A ordem dos vetores para um pedido: as declarações da mais perto para a
/// mais longe e, de cada arquivo, a declaração mais perto dele.
#[derive(Debug, Clone, Default)]
pub(super) struct Meaning {
    /// As declarações mais perto do pedido, até a profundidade pedida.
    pub decls: Vec<i64>,
    /// A declaração mais perto do pedido em cada arquivo, para os primeiros
    /// arquivos que têm alguma.
    pub best: HashMap<i64, i64>,
}

impl Meaning {
    /// A ordem dos vetores do texto `text` sobre as declarações de
    /// `file_of` (declaração para arquivo), com até `depth` declarações e
    /// `depth` arquivos. Vazia no mapa sem vetores.
    pub(super) fn of(conn: &Connection, text: &str, file_of: &HashMap<i64, i64>, depth: usize) -> Result<Self> {
        let mut out = Self::default();
        for similar in ranked_declarations(conn, text)? {
            let Some(&file) = file_of.get(&similar.id) else { continue };
            if out.decls.len() < depth {
                out.decls.push(similar.id);
            }
            if out.best.len() < depth {
                out.best.entry(file).or_insert(similar.id);
            }
            if out.decls.len() >= depth && out.best.len() >= depth {
                break;
            }
        }
        Ok(out)
    }

    /// Se o pedido não tem ordem de vetores.
    pub(super) fn is_empty(&self) -> bool {
        self.decls.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::search::{fuse, VECTOR_WEIGHT};
    use crate::domain::triage::Mark;
    use crate::io::map_meaning::fill_at;
    use crate::io::map_order::{ordered, ordered_with, Check};
    use crate::io::map_lists::{decl_files, ranked_files_near, sources_near};
    use crate::io::map_search::candidates_at;
    use crate::io::map_triage::triage_at;
    use crate::io::project_map::{self as store, model_path, open_existing};
    use serde_json::{json, Value};
    use tempfile::TempDir;

    fn both() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    fn english() -> Languages {
        Languages::new(["en-US"])
    }

    fn function(name: &str, doc: &str) -> Value {
        json!({"kind": "function", "name": name, "line": 1, "end_line": 5,
               "signature": format!("pub fn {name}()"), "doc": doc})
    }

    fn module(path: &str, name: &str, doc: &str) -> Value {
        json!({"path": path, "declarations": [function(name, doc)]})
    }

    /// Os arquivos que não dizem nada dos pedidos dos testes.
    fn others() -> Vec<Value> {
        vec![
            module("src/invoice.rs", "charge_invoice", "Charge the customer invoice with the tax."),
            module("src/config.rs", "parse_config", "Parse the configuration file."),
            module("src/users.rs", "list_users", "List the users of the account."),
        ]
    }

    /// Um projeto com os módulos `modules` gravado como o scan grava, sem os
    /// vetores do sentido.
    fn saved(modules: Vec<Value>, languages: &Languages) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        store::save_at(&model_path(dir.path()), &json!({ "modules": modules }), "scan 1", languages).unwrap();
        dir
    }

    /// O mesmo, com os vetores do sentido gravados.
    fn filled(modules: Vec<Value>, languages: &Languages) -> TempDir {
        let dir = saved(modules, languages);
        fill_at(&model_path(dir.path()), dir.path()).unwrap();
        dir
    }

    /// Um projeto em inglês que só escreve `remove` para apagar.
    fn remove_project(languages: &Languages) -> TempDir {
        let mut modules = vec![module("src/dirs.rs", "remove_dir", "Remove the directory and everything inside it.")];
        modules.extend(others());
        filled(modules, languages)
    }

    /// Um projeto cujo arquivo de pagamento só escreve `installments` em inglês.
    fn installments_project(languages: &Languages) -> TempDir {
        let mut modules = vec![module("src/pay.rs", "splitInstallments", "Splits the invoice total into monthly payments.")];
        modules.extend(others());
        filled(modules, languages)
    }

    /// Os caminhos da resposta das palavras de `query`, sem o sentido e com ele.
    fn answers(dir: &TempDir, query: &str, languages: &Languages) -> (Vec<String>, Vec<String>) {
        let db = open_existing(&model_path(dir.path())).unwrap();
        let paths = |ordered: crate::io::map_order::Ordered| ordered.files.into_iter().map(|f| f.path).collect::<Vec<_>>();
        let today = ordered_with(db.conn(), Check::Off, &Sense::off(), query, "", languages).unwrap();
        let sense = Sense::read(db.conn(), languages, (query, ""), false).unwrap();
        let with = ordered_with(db.conn(), Check::Off, &sense, query, "", languages).unwrap();
        (paths(today), paths(with))
    }

    fn pool_of(dir: &TempDir, word: &str, languages: &Languages) -> Option<Pool> {
        let db = open_existing(&model_path(dir.path())).unwrap();
        let mut pools = Pools::prepare(db.conn()).unwrap();
        let own = Normalizer::new(languages).word_forms(word);
        pools.other_of(&own).unwrap()
    }

    /// Um sinônimo, na mesma língua, acha a função do projeto que só escreve
    /// a outra palavra: `delete` acha `remove_dir` (cosseno 0,703); só pelas
    /// palavras escritas nada é achado.
    #[test]
    fn a_synonym_finds_the_function_of_a_project_that_only_writes_the_other_word() {
        let dir = remove_project(&english());
        let (today, with) = answers(&dir, "delete", &english());
        assert!(today.is_empty(), "the written words find nothing: {today:?}");
        assert_eq!(with.first().map(String::as_str), Some("src/dirs.rs"), "{with:?}");
    }

    /// A palavra que o mapa escreve não ganha forma vizinha: `remove` está em
    /// `remove_dir`, então o pedido `remove` não traz `delete_file` como
    /// sinônimo (cosseno 0,703), e a função que escreve a palavra do pedido
    /// nunca perde lugar para a que só escreve a vizinha.
    #[test]
    fn a_word_the_map_writes_gets_no_neighbor_form() {
        let mut modules = vec![
            module("src/dirs.rs", "remove_dir", "Remove the directory and everything inside it."),
            module("src/files.rs", "delete_file", "Delete the file from the disk."),
        ];
        modules.extend(others());
        let dir = filled(modules, &english());
        let (today, with) = answers(&dir, "remove", &english());
        assert_eq!(today, ["src/dirs.rs"], "{today:?}");
        assert_eq!(with, today, "the neighbor `delete` does not join a word the map writes: {with:?}");
    }

    /// O modelo só é usado por palavra que o mapa não escreve: o pedido cujas
    /// palavras o mapa todo escreve não dá nenhuma palavra ao modelo.
    #[test]
    fn the_model_is_asked_only_for_the_words_the_map_does_not_write() {
        let dir = remove_project(&english());
        let db = open_existing(&model_path(dir.path())).unwrap();
        let before = tuning::encoded();
        let written = Near::of(db.conn(), &english(), "remove directory").unwrap();
        assert!(written.is_empty());
        assert_eq!(tuning::encoded(), before, "every word is written by the map: nothing goes to the model");
        let unwritten = Near::of(db.conn(), &english(), "remove delete").unwrap();
        assert!(!unwritten.is_empty());
        assert_eq!(tuning::encoded(), before + 1, "only `delete` goes to the model");
    }

    /// A forma vizinha vale a metade da que a pergunta escreveu: o projeto que
    /// escreve `remove` dá ao pedido `delete` a metade da nota que o projeto
    /// igual que escreve `delete` dá.
    #[test]
    fn a_synonym_weighs_half_of_the_word_the_request_wrote() {
        let mut written = vec![module("src/dirs.rs", "delete_dir", "Delete the directory and everything inside it.")];
        written.extend(others());
        let written = filled(written, &english());
        let synonym = remove_project(&english());
        let score = |dir: &TempDir| {
            let db = open_existing(&model_path(dir.path())).unwrap();
            let sense = Sense::read(db.conn(), &english(), ("delete", ""), false).unwrap();
            let found = ranked_files_near(db.conn(), "delete", &english(), 10, &sense.near).unwrap();
            assert_eq!(found[0].path, "src/dirs.rs", "{found:?}");
            found[0].score
        };
        let (whole, half) = (score(&written), score(&synonym));
        assert!(half > 0 && (2 * half).abs_diff(whole) <= 2, "half of {whole} expected, got {half}");
    }

    fn neighbor(word: &str, cosine: f32) -> Neighbor {
        Neighbor { word: word.to_string(), forms: word.to_string(), cosine }
    }

    /// Só os três primeiros sinônimos com cosseno de pelo menos 0,6 entram, o
    /// de 0,59 fica fora, e a palavra que a pergunta já traz não conta.
    #[test]
    fn at_most_three_synonyms_of_cosine_point_six_or_more_join_a_word() {
        let neighbors = [
            neighbor("asked", 0.99),
            neighbor("alpha", 0.9),
            neighbor("bravo", 0.8),
            neighbor("charlie", 0.7),
            neighbor("delta", 0.65),
            neighbor("echo", 0.6),
            neighbor("foxtrot", 0.59),
        ];
        let own = vec!["asked".to_string()];
        assert_eq!(choose(&neighbors, &own, None).unwrap(), ["alpha", "bravo", "charlie"]);
        assert!(choose(&neighbors[6..], &own, None).unwrap().is_empty(), "0.59 is below the floor");
        assert_eq!(choose(&neighbors[5..6], &own, None).unwrap(), ["echo"], "0.6 is enough");
    }

    /// Palavra de ligação nunca ganha vizinha, ainda que o projeto tenha uma
    /// palavra perto dela: `all` está a 0,468 de `everything`, acima do limite
    /// entre línguas, e o código do projeto escreve `everything`.
    #[test]
    fn a_function_word_never_gets_a_neighbor() {
        let mut modules = vec![module("src/dirs.rs", "remove_everything", "Remove the tree.")];
        modules.extend(others());
        let dir = filled(modules, &both());
        let db = open_existing(&model_path(dir.path())).unwrap();
        assert!(Near::of(db.conn(), &both(), "all").unwrap().is_empty());
        assert!(Near::of(db.conn(), &both(), "para the of all").unwrap().is_empty());
    }

    /// A palavra em português que o código não escreve acha o nome em inglês
    /// pelo limite próprio entre línguas: `excluir` (0,443 de `remove`) acha
    /// `remove_dir`, `parcelamento` (0,427 de `installments`) acha
    /// `splitInstallments`, e `parcela` (0,253) fica abaixo do limite de 0,35.
    #[test]
    fn a_portuguese_word_reaches_the_declaration_whose_name_is_english() {
        let dirs = remove_project(&both());
        let (today, with) = answers(&dirs, "excluir", &both());
        assert!(today.is_empty(), "{today:?}");
        assert_eq!(with.first().map(String::as_str), Some("src/dirs.rs"), "{with:?}");

        let pay = installments_project(&both());
        let (today, with) = answers(&pay, "parcelamento", &both());
        assert!(today.is_empty(), "{today:?}");
        assert_eq!(with.first().map(String::as_str), Some("src/pay.rs"), "{with:?}");

        let (_, below) = answers(&pay, "parcela", &both());
        assert!(below.is_empty(), "0.253 is under the cross-language limit: {below:?}");
        assert!((OTHER_LANGUAGE_COSINE - 0.35).abs() < f32::EPSILON);
    }

    /// O projeto de uma língua só responde como hoje: sem a travessia,
    /// `excluir` (0,443, abaixo do 0,6 dos sinônimos) não acha nada.
    #[test]
    fn a_project_of_one_language_answers_as_it_does_today() {
        let dir = remove_project(&english());
        let (today, with) = answers(&dir, "excluir", &english());
        assert!(today.is_empty() && with.is_empty(), "{today:?} {with:?}");
        let db = open_existing(&model_path(dir.path())).unwrap();
        assert!(Near::of(db.conn(), &english(), "excluir").unwrap().is_empty());
        let crossing = Near::of(db.conn(), &both(), "excluir").unwrap();
        assert!(!crossing.is_empty(), "the same project with two languages crosses");
    }

    /// A língua da palavra sai de onde o índice a tem: a que o código não
    /// escreve procura as vizinhas no código, a que só o código escreve, no
    /// texto, e a que os dois escrevem já está nas duas línguas e não
    /// atravessa.
    #[test]
    fn the_pool_of_the_other_language_follows_where_the_index_holds_the_word() {
        let dir = remove_project(&both());
        assert_eq!(pool_of(&dir, "excluir", &both()), Some(Pool::Code), "the code does not write it");
        assert_eq!(pool_of(&dir, "directory", &both()), Some(Pool::Code), "only the doc writes it");
        assert_eq!(pool_of(&dir, "dir", &both()), Some(Pool::Text), "only the code writes it");
        assert_eq!(pool_of(&dir, "remove", &both()), None, "the code and the doc write it");
    }

    /// Onde nenhuma palavra casa, a lista de candidatos vem da ordem dos
    /// vetores: o pedido inteiro em português põe `remove_dir` na frente; sem
    /// o sentido a lista é vazia.
    #[test]
    fn the_candidate_list_takes_the_order_of_the_vectors_where_no_word_matches() {
        let dir = remove_project(&both());
        let request = "apagar a pasta e tudo dentro dela";
        let db = open_existing(&model_path(dir.path())).unwrap();
        let today = ordered_with(db.conn(), Check::Off, &Sense::off(), request, "", &both()).unwrap();
        assert!(today.list.is_empty(), "{:?}", today.list);
        let found = candidates_at(&model_path(dir.path()), request, "", &both(), 100).unwrap();
        assert_eq!(found.candidates.first().map(|c| c.name.as_str()), Some("remove_dir"), "{:?}", found.whole);
        assert_eq!(found.candidates.len(), 4, "every declaration is a candidate, the nearest first");
    }

    /// A ordem dos vetores nunca desfaz o "não achei": o pedido que só o
    /// sentido do texto inteiro acha continua sem achado nas palavras, e o
    /// que uma forma vizinha acha deixa de ser "não achei".
    #[test]
    fn the_not_found_notice_falls_only_for_the_words_and_their_neighbors() {
        let dir = remove_project(&both());
        let map = model_path(dir.path());
        let vectors_only = triage_at(&map, ("apagar a pasta e tudo dentro dela", ""), &both(), 5).unwrap();
        assert_eq!((vectors_only.grade, vectors_only.mark()), (0, Mark::NotFound), "{vectors_only:?}");
        assert!(vectors_only.files.is_empty(), "{vectors_only:?}");
        let by_neighbor = triage_at(&map, ("excluir", ""), &both(), 5).unwrap();
        assert_ne!(by_neighbor.mark(), Mark::NotFound, "{by_neighbor:?}");
        assert_eq!(by_neighbor.files.first().map(|f| f.path.as_str()), Some("src/dirs.rs"), "{by_neighbor:?}");
    }

    /// A lista de candidatos é a cabeça da resposta seguida da ordem fundida:
    /// as palavras com peso 1 e os vetores com 0,5 por posição recíproca.
    #[test]
    fn the_candidate_list_after_the_head_is_the_fusion_of_the_words_and_the_vectors() {
        let mut modules: Vec<Value> = (1..=7)
            .map(|n| module(&format!("src/erase{n}.rs"), &format!("erase{n}"), &format!("Erase the item number {n}.")))
            .collect();
        modules.extend(others());
        let dir = filled(modules, &english());
        let db = open_existing(&model_path(dir.path())).unwrap();
        let (query, languages) = ("erase the configuration file", english());
        let sense = Sense::read(db.conn(), &languages, (query, ""), true).unwrap();
        let with = ordered_with(db.conn(), Check::Off, &sense, query, "", &languages).unwrap();
        let words = ordered_with(db.conn(), Check::Off, &Sense { near: sense.near.clone(), meaning: false }, query, "", &languages)
            .unwrap();
        assert_eq!(with.files, words.files, "the answer keeps the order of the words");
        let head = with.files.len().min(crate::domain::search::TOP);
        assert_eq!(with.list[..head], words.list[..head], "the head is the same");
        let file_of: HashMap<i64, i64> = decl_files(db.conn()).unwrap().into_iter().collect();
        let meaning = Meaning::of(db.conn(), query, &file_of, 200).unwrap();
        let whole = sources_near(db.conn(), query, "", &languages, &sense.near).unwrap().whole();
        let fused = fuse(&whole, &meaning.decls, VECTOR_WEIGHT);
        let expected: Vec<i64> = words.list[..head].iter().copied().chain(fused.into_iter().filter(|id| !words.list[..head].contains(id))).collect();
        assert_eq!(with.list, expected);
        assert_ne!(with.list, words.list, "the vectors change the order of the tail");
    }

    /// A lista de candidatos guarda inteira a ordem das palavras que a
    /// pergunta escreve: a ordem dos vetores e a forma vizinha só acrescentam,
    /// depois delas, o que as palavras não acharam, e nunca passam um arquivo
    /// que as palavras acharam.
    #[test]
    fn the_candidate_list_keeps_the_written_words_first_and_the_sense_only_adds_after() {
        let mut modules: Vec<Value> = (1..=7)
            .map(|n| module(&format!("src/erase{n}.rs"), &format!("erase{n}"), &format!("Erase the item number {n}.")))
            .collect();
        modules.extend(others());
        let dir = filled(modules, &english());
        let db = open_existing(&model_path(dir.path())).unwrap();
        let (query, languages) = ("erase the configuration file", english());
        let words = ordered_with(db.conn(), Check::Off, &Sense::off(), query, "", &languages).unwrap();
        let sense = Sense::read(db.conn(), &languages, (query, ""), true).unwrap();
        let fused = ordered_with(db.conn(), Check::Off, &sense, query, "", &languages).unwrap();
        assert_ne!(
            fused.list[..words.list.len()],
            words.list[..],
            "the fixture only proves the rule if the vectors would reorder what the words found"
        );
        let list = ordered(db.conn(), Check::Off, query, "", &languages).unwrap().list;
        assert_eq!(list[..words.list.len()], words.list[..], "the words keep their order in the list");
        let rest: HashSet<i64> = list[words.list.len()..].iter().copied().collect();
        assert!(rest.iter().all(|id| !words.list.contains(id)), "nothing the words found repeats after them");
    }

    /// O mapa sem vetores responde como sempre: nenhuma forma vizinha, nenhuma
    /// ordem de vetores, a mesma lista e a mesma resposta.
    #[test]
    fn a_map_without_vectors_answers_as_before() {
        let mut modules = vec![module("src/dirs.rs", "remove_dir", "Remove the directory and everything inside it.")];
        modules.extend(others());
        let dir = saved(modules, &both());
        let db = open_existing(&model_path(dir.path())).unwrap();
        assert!(Near::of(db.conn(), &both(), "excluir delete").unwrap().is_empty());
        let file_of: HashMap<i64, i64> = decl_files(db.conn()).unwrap().into_iter().collect();
        assert!(Meaning::of(db.conn(), "excluir a pasta", &file_of, 200).unwrap().is_empty());
        for query in ["remove", "excluir", "apagar a pasta e tudo dentro dela"] {
            let with = ordered(db.conn(), Check::Off, query, "", &both()).unwrap();
            let today = ordered_with(db.conn(), Check::Off, &Sense::off(), query, "", &both()).unwrap();
            assert_eq!((with.list, with.files), (today.list, today.files), "{query}");
        }
        let notice = triage_at(&model_path(dir.path()), ("excluir", ""), &both(), 5).unwrap();
        assert_eq!((notice.grade, notice.mark()), (0, Mark::NotFound), "{notice:?}");
    }

    /// A resposta cravada pelas palavras é a mesma com e sem vetores: `remove`
    /// crava `remove_dir`, e o `delete_file` que o sentido põe perto de
    /// `remove` (cosseno 0,703) não entra na resposta nem muda o grau.
    #[test]
    fn a_pinned_answer_is_the_same_with_and_without_vectors() {
        let mut modules = vec![
            module("src/dirs.rs", "remove_dir", ""),
            module("src/files.rs", "delete_file", "Delete the file from the disk."),
        ];
        modules.extend((0..12).map(|n| module(&format!("src/other{n}.rs"), &format!("do_it{n}"), "something quite different")));
        let plain = saved(modules.clone(), &english());
        let sensed = filled(modules, &english());
        let ask = |dir: &TempDir| triage_at(&model_path(dir.path()), ("remove", ""), &english(), 5).unwrap();
        let (plain, sensed) = (ask(&plain), ask(&sensed));
        assert_eq!((plain.grade, plain.mark()), (5, Mark::Pinned), "{plain:?}");
        assert_eq!((sensed.grade, sensed.mark()), (5, Mark::Pinned), "{sensed:?}");
        assert_eq!(plain.files, sensed.files);
        assert_eq!(sensed.files.len(), 1, "{sensed:?}");
    }
}
