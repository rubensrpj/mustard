//! `search` — a busca por palavras sobre o campo `search` dos eventos e das
//! lições, sobre os arquivos do mapa e sobre as skills.
//!
//! A pergunta e cada documento chegam em palavras, cada uma com as suas
//! formas, pela normalização do projeto (`domain::normalize`), nas línguas
//! dele. A busca monta, a cada consulta, um índice invertido em memória: para
//! cada forma, os documentos que a têm. A nota de cada documento é o BM25 de
//! `domain::ranking`, pesado pela raridade da forma: cada palavra da pergunta
//! soma a forma que casa mais forte no documento, nunca a soma das formas, e o
//! tamanho do documento conta palavras, não formas. Voltam só as respostas
//! mais fortes.
//!
//! Função pura: sem disco e sem relógio. A mesma entrada dá sempre a mesma
//! resposta, na mesma ordem.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::ranking::{avgdl_x1024, bm25_x1024_default, idf_x1024, SCALE};

/// Quantas respostas a busca devolve.
pub const TOP: usize = 5;

/// Uma resposta: o número do documento (evento ou lição) e a nota ×1024.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub id: u64,
    pub score: u64,
}

/// O índice invertido em memória, montado sobre as palavras de cada
/// documento, cada uma com as suas formas.
#[derive(Debug, Clone, Default)]
pub struct SearchIndex {
    /// O número de cada documento, na ordem recebida.
    ids: Vec<u64>,
    /// Quantas palavras cada documento tem.
    lens: Vec<usize>,
    /// Forma -> (posição do documento, quantas palavras dele têm a forma).
    postings: BTreeMap<String, Vec<(usize, usize)>>,
    /// O tamanho médio dos documentos, ×1024.
    avgdl: u64,
}

impl SearchIndex {
    /// Monta o índice. Cada documento é o número dele e as palavras dele, cada
    /// uma com as suas formas; a palavra com as mesmas formas de outra conta
    /// uma vez.
    #[must_use]
    pub fn build(docs: impl IntoIterator<Item = (u64, Vec<Vec<String>>)>) -> Self {
        let mut index = Self::default();
        for (pos, (id, words)) in docs.into_iter().enumerate() {
            let mut seen: HashSet<&[String]> = HashSet::new();
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            let mut len = 0usize;
            for word in &words {
                if word.is_empty() || !seen.insert(word.as_slice()) {
                    continue;
                }
                len += 1;
                let forms: BTreeSet<&str> = word.iter().map(String::as_str).collect();
                for form in forms {
                    *counts.entry(form).or_insert(0) += 1;
                }
            }
            for (form, tf) in counts {
                index.postings.entry(form.to_string()).or_default().push((pos, tf));
            }
            index.ids.push(id);
            index.lens.push(len);
        }
        index.avgdl = avgdl_x1024(index.lens.iter().sum(), index.ids.len());
        index
    }

    /// As `limit` respostas mais fortes para as palavras da pergunta `query`,
    /// cada uma com as suas formas: nota decrescente e, no empate, o número
    /// menor primeiro. Só entra documento que casa com pelo menos uma palavra.
    ///
    /// Cada palavra soma a nota da forma que casa mais forte no documento. A
    /// forma pesa `1 + IDF`: todo casamento conta, e a forma rara conta mais.
    /// O IDF sozinho daria zero a uma forma presente em todos os documentos, e
    /// uma lição sozinha no banco nunca seria achada.
    #[must_use]
    pub fn top(&self, query: &[Vec<String>], limit: usize) -> Vec<Hit> {
        let n = self.ids.len();
        let mut scores = vec![0u64; n];
        let mut best = vec![0u64; n];
        let mut touched: Vec<usize> = Vec::new();
        let words: BTreeSet<BTreeSet<&str>> =
            query.iter().map(|word| word.iter().map(String::as_str).collect()).collect();
        for word in words {
            for form in word {
                let Some(postings) = self.postings.get(form) else { continue };
                let weight = SCALE.saturating_add(idf_x1024(postings.len(), n));
                for &(pos, tf) in postings {
                    let bm25 = bm25_x1024_default(tf, self.lens[pos], self.avgdl);
                    let score = weight.saturating_mul(bm25) / SCALE;
                    if best[pos] == 0 {
                        touched.push(pos);
                    }
                    best[pos] = best[pos].max(score);
                }
            }
            for pos in touched.drain(..) {
                scores[pos] = scores[pos].saturating_add(best[pos]);
                best[pos] = 0;
            }
        }
        let mut hits: Vec<Hit> = scores
            .into_iter()
            .enumerate()
            .filter(|(_, score)| *score > 0)
            .map(|(pos, score)| Hit { id: self.ids[pos], score })
            .collect();
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
        hits.truncate(limit);
        hits
    }
}

/// Quantas palavras da pergunta `query` têm alguma forma no documento `doc`.
#[must_use]
pub fn shared_words(query: &[Vec<String>], doc: &[Vec<String>]) -> usize {
    let forms: HashSet<&str> = doc.iter().flatten().map(String::as_str).collect();
    let words: BTreeSet<BTreeSet<&str>> =
        query.iter().map(|word| word.iter().map(String::as_str).collect()).collect();
    words.into_iter().filter(|word| word.iter().any(|form| forms.contains(form))).count()
}

/// Monta o índice sobre o campo `search` de cada documento e devolve as
/// [`TOP`] respostas mais fortes para o pedido, nas línguas `languages`.
#[must_use]
pub fn search<'a>(docs: impl IntoIterator<Item = (u64, &'a str)>, query: &str, languages: &Languages) -> Vec<Hit> {
    let mut normalizer = Normalizer::new(languages);
    let docs: Vec<(u64, Vec<Vec<String>>)> = docs.into_iter().map(|(id, field)| (id, normalizer.forms(field))).collect();
    SearchIndex::build(docs).top(&normalizer.query(query), TOP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::search_field;

    /// O `search` como o binário grava: as palavras do texto e das chaves.
    fn doc(text: &str, keys: &[&str]) -> String {
        search_field(Some(text), keys)
    }

    fn ids(hits: &[Hit]) -> Vec<u64> {
        hits.iter().map(|h| h.id).collect()
    }

    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    fn found(docs: &[(u64, String)], query: &str) -> Vec<Hit> {
        search(docs.iter().map(|(id, s)| (*id, s.as_str())), query, &languages())
    }

    /// Um documento de palavras com uma forma só cada.
    fn single(words: &str) -> Vec<Vec<String>> {
        words.split(' ').map(|word| vec![word.to_string()]).collect()
    }

    /// A lição gravada com a chave "apagar" é achada por "apagando a pasta",
    /// sozinha no banco e no meio de outras oito.
    #[test]
    fn a_lesson_keyed_apagar_is_among_the_top_five_for_apagando_a_pasta() {
        let lesson = (7, doc("A trava de comandos confere o programa, nunca o texto entre aspas.", &["trava", "apagar"]));
        let alone = found(std::slice::from_ref(&lesson), "apagando a pasta");
        assert_eq!(ids(&alone), [7], "{alone:?}");

        let mut bank = vec![
            (1, doc("O cargo não está no PATH do shell; chame o caminho inteiro.", &["cargo", "PATH"])),
            (2, doc("Gancho nunca entra em pânico nem barra a sessão por erro próprio.", &["gancho", "pânico"])),
            (3, doc("A pasta temporária some depois do teste.", &["pasta", "temporária"])),
            (4, doc("O título do pull request tem até 60 caracteres.", &["título", "limite"])),
            (5, doc("A página é publicada só nos marcos.", &["página", "publicação"])),
            (6, doc("Uma pasta de spec tem o arquivo de eventos.", &["pasta", "spec"])),
            (8, doc("Commit sem coautoria e sem link.", &["commit", "coautoria"])),
            (9, doc("Os testes gravam sempre numa pasta temporária.", &["teste", "pasta"])),
        ];
        bank.push(lesson);
        let hits = found(&bank, "apagando a pasta");
        assert!(hits.len() <= TOP, "{hits:?}");
        assert!(ids(&hits).contains(&7), "the lesson keyed apagar is in the top five: {hits:?}");
    }

    /// Com um documento só, o termo está em todos os documentos, e mesmo
    /// assim o casamento dá nota.
    #[test]
    fn a_single_document_that_matches_still_scores() {
        let docs = [(1, doc("Apagar a pasta inteira.", &[]))];
        let hits = found(&docs, "apagar");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].score > 0, "{hits:?}");
    }

    /// Dois documentos do mesmo tamanho, cada um com um termo do pedido: o do
    /// termo raro vem antes do termo que aparece em quase todos.
    #[test]
    fn a_rare_term_outweighs_a_common_one() {
        let docs = [
            (1, single("trav comum")),
            (2, single("cofr outro")),
            (3, single("comum mais")),
            (4, single("comum menos")),
            (5, single("comum ainda")),
        ];
        let index = SearchIndex::build(docs);
        let hits = index.top(&[vec!["comum".to_string()], vec!["cofr".to_string()]], TOP);
        assert_eq!(hits[0].id, 2, "the rare term wins: {hits:?}");
        assert!(hits[0].score > hits[1].score, "{hits:?}");
    }

    #[test]
    fn function_words_alone_find_nothing() {
        let docs = [(1, doc("A pasta de testes é para o time.", &["pasta"]))];
        assert!(Normalizer::new(&languages()).query("a de para o the of").is_empty());
        assert!(found(&docs, "a de para o").is_empty());
    }

    /// A raiz de "somar" não é a raiz da palavra funcional inglesa "some":
    /// achar "somar" não pode depender de um texto que só tem "some", e tem
    /// que achar o texto que fala em "soma".
    #[test]
    fn somar_does_not_match_the_english_word_some() {
        let docs = [
            (1, doc("There is some pasta left in the pot.", &[])),
            (2, doc("A soma dos valores está errada.", &[])),
        ];
        let hits = found(&docs, "somar");
        assert_eq!(ids(&hits), [2], "{hits:?}");
    }

    #[test]
    fn no_more_than_five_hits_come_back() {
        let docs: Vec<(u64, String)> = (1..=8).map(|i| (i, doc(&format!("Apagar a pasta {i}."), &[]))).collect();
        let hits = found(&docs, "apagando");
        assert_eq!(hits.len(), TOP, "{hits:?}");
    }

    /// Documentos iguais empatam, e o número menor vem primeiro, qualquer que
    /// seja a ordem de entrada; duas rodadas dão a mesma resposta.
    #[test]
    fn a_tie_keeps_the_lower_number_first_and_the_order_is_the_same_every_run() {
        let same = doc("Apagar a pasta.", &[]);
        let docs = [(9, same.clone()), (3, same.clone()), (6, same)];
        let first = found(&docs, "apagando a pasta");
        assert_eq!(ids(&first), [3, 6, 9]);
        assert!(first.windows(2).all(|w| w[0].score == w[1].score), "{first:?}");
        assert_eq!(first, found(&docs, "apagando a pasta"));
    }

    /// Cada palavra da pergunta soma só a forma que casa mais forte: o
    /// documento que tem as duas formas da palavra empata com o que tem uma
    /// só, e perde para o que casa duas palavras.
    #[test]
    fn a_word_of_the_question_counts_its_best_form_never_the_sum_of_its_forms() {
        let forms = |list: &[&str]| -> Vec<String> { list.iter().map(|f| f.to_string()).collect() };
        let docs = [
            (1, vec![forms(&["usuari", "usuario"]), forms(&["outr"])]),
            (2, vec![forms(&["usuari"]), forms(&["grav"])]),
            (3, vec![forms(&["usuario"]), forms(&["mais"])]),
        ];
        let index = SearchIndex::build(docs);
        let word = forms(&["usuari", "usuario"]);
        let hits = index.top(std::slice::from_ref(&word), TOP);
        assert_eq!(ids(&hits), [1, 2, 3], "{hits:?}");
        assert!(hits.windows(2).all(|w| w[0].score == w[1].score), "two forms weigh as the best one: {hits:?}");
        let both = index.top(&[word, forms(&["grav"])], TOP);
        assert_eq!(ids(&both)[0], 2, "two words beat two forms of one word: {both:?}");
    }

    /// O tamanho do documento conta palavras, e não formas: a palavra com
    /// três formas pesa no tamanho como a palavra com uma.
    #[test]
    fn the_length_of_a_document_counts_words_not_forms() {
        let many = vec![vec!["a1".to_string(), "a2".to_string(), "a3".to_string()], vec!["alvo".to_string()]];
        let one = vec![vec!["b1".to_string()], vec!["alvo".to_string()]];
        let index = SearchIndex::build([(1, many), (2, one)]);
        let hits = index.top(&[vec!["alvo".to_string()]], TOP);
        assert_eq!(ids(&hits), [1, 2], "{hits:?}");
        assert_eq!(hits[0].score, hits[1].score, "same number of words, same score: {hits:?}");
    }
}
