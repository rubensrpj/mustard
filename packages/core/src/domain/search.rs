//! `search` — a busca por palavras sobre o campo `search` dos eventos e das
//! lições.
//!
//! O `search` de cada linha já vem reduzido pelo binário (minúsculas, raiz de
//! cada palavra, sem acento, sem repetição). A busca monta, a cada consulta, um
//! índice invertido em memória: para cada raiz, os documentos que a têm. A nota
//! de cada documento é o BM25 de `domain::ranking`, somado termo a termo e
//! pesado pela raridade do termo, e voltam só as respostas mais fortes.
//!
//! O pedido passa pelo mesmo redutor do `search` (`spec_events::search_terms`),
//! então "apagando" acha a lição gravada com a chave "apagar". As palavras
//! funcionais de português e de inglês ("a", "de", "the") saem do pedido: sem
//! isso, o "a" de "apagando a pasta" casaria qualquer texto com um "a".
//!
//! Função pura: sem disco e sem relógio. A mesma entrada dá sempre a mesma
//! resposta, na mesma ordem.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::ranking::{avgdl_x1024, bm25_x1024_default, idf_x1024, SCALE};
use crate::domain::spec_events::search_terms;
use crate::domain::text::{self, FUNCTION_WORDS_EN, FUNCTION_WORDS_PT};

/// Quantas respostas a busca devolve.
pub const TOP: usize = 5;

/// Uma resposta: o número do documento (evento ou lição) e a nota ×1024.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub id: u64,
    pub score: u64,
}

/// O índice invertido em memória, montado sobre o campo `search` de cada
/// documento.
#[derive(Debug, Clone, Default)]
pub struct SearchIndex {
    /// O número de cada documento, na ordem recebida.
    ids: Vec<u64>,
    /// Quantas raízes o `search` de cada documento tem.
    lens: Vec<usize>,
    /// Raiz -> (posição do documento, quantas vezes a raiz aparece nele).
    postings: BTreeMap<String, Vec<(usize, usize)>>,
    /// O tamanho médio dos documentos, ×1024.
    avgdl: u64,
}

impl SearchIndex {
    /// Monta o índice. Cada documento é o número dele e o campo `search`, que
    /// já vem reduzido e é quebrado por espaço.
    #[must_use]
    pub fn build<'a>(docs: impl IntoIterator<Item = (u64, &'a str)>) -> Self {
        let mut index = Self::default();
        for (pos, (id, search)) in docs.into_iter().enumerate() {
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            let mut len = 0usize;
            for root in search.split(' ').filter(|w| !w.is_empty()) {
                *counts.entry(root).or_insert(0) += 1;
                len += 1;
            }
            for (root, tf) in counts {
                index.postings.entry(root.to_string()).or_default().push((pos, tf));
            }
            index.ids.push(id);
            index.lens.push(len);
        }
        index.avgdl = avgdl_x1024(index.lens.iter().sum(), index.ids.len());
        index
    }

    /// As `limit` respostas mais fortes para as raízes `terms`: nota
    /// decrescente e, no empate, o número menor primeiro. Só entra documento
    /// que casa com pelo menos um termo.
    ///
    /// Cada termo pesa `1 + IDF`: todo casamento conta, e o termo raro conta
    /// mais. O IDF sozinho daria zero a um termo presente em todos os
    /// documentos, e uma lição sozinha no banco nunca seria achada.
    #[must_use]
    pub fn top(&self, terms: &[String], limit: usize) -> Vec<Hit> {
        let n = self.ids.len();
        let mut scores = vec![0u64; n];
        let unique: BTreeSet<&str> = terms.iter().map(String::as_str).collect();
        for term in unique {
            let Some(postings) = self.postings.get(term) else { continue };
            let weight = SCALE.saturating_add(idf_x1024(postings.len(), n));
            for &(pos, tf) in postings {
                let bm25 = bm25_x1024_default(tf, self.lens[pos], self.avgdl);
                scores[pos] = scores[pos].saturating_add(weight.saturating_mul(bm25) / SCALE);
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

/// As raízes de um pedido para a busca: as de `search_terms`, sobre o pedido
/// já sem as palavras funcionais de português e de inglês — cortadas pelo
/// texto delas, antes do radical, para não confundir a raiz de uma com a de
/// uma palavra de conteúdo parecida (a raiz de "some" nunca é a de "somar").
/// Um pedido só com palavras funcionais vira vazio.
#[must_use]
pub fn query_terms(query: &str) -> Vec<String> {
    let lower = query.to_lowercase();
    let kept: Vec<&str> =
        text::words(&lower).filter(|w| !FUNCTION_WORDS_PT.contains(w) && !FUNCTION_WORDS_EN.contains(w)).collect();
    if kept.is_empty() {
        return Vec::new();
    }
    search_terms(&kept.join(" "))
}

/// Monta o índice e devolve as [`TOP`] respostas mais fortes para o pedido.
#[must_use]
pub fn search<'a>(docs: impl IntoIterator<Item = (u64, &'a str)>, query: &str) -> Vec<Hit> {
    SearchIndex::build(docs).top(&query_terms(query), TOP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::search_field;

    /// O `search` como o binário grava: as raízes do texto e das chaves.
    fn doc(text: &str, keys: &[&str]) -> String {
        search_field(Some(text), keys)
    }

    fn ids(hits: &[Hit]) -> Vec<u64> {
        hits.iter().map(|h| h.id).collect()
    }

    fn found(docs: &[(u64, String)], query: &str) -> Vec<Hit> {
        search(docs.iter().map(|(id, s)| (*id, s.as_str())), query)
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
            (1, "trav comum".to_string()),
            (2, "cofr outro".to_string()),
            (3, "comum mais".to_string()),
            (4, "comum menos".to_string()),
            (5, "comum ainda".to_string()),
        ];
        let index = SearchIndex::build(docs.iter().map(|(id, s)| (*id, s.as_str())));
        let hits = index.top(&["comum".to_string(), "cofr".to_string()], TOP);
        assert_eq!(hits[0].id, 2, "the rare term wins: {hits:?}");
        assert!(hits[0].score > hits[1].score, "{hits:?}");
    }

    #[test]
    fn function_words_alone_find_nothing() {
        let docs = [(1, doc("A pasta de testes é para o time.", &["pasta"]))];
        assert!(query_terms("a de para o the of").is_empty());
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
}
