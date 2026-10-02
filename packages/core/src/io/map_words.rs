//! `map_words` — as palavras de uma pergunta como o índice do mapa as lê.
//!
//! A pergunta é quebrada em palavras (o nome colado se separa), as de ligação
//! saem, e cada palavra leva as formas da normalização nas línguas do
//! projeto e as formas como o índice as grava. A triagem, a conferência dos
//! primeiros candidatos e a consulta agrupada partem das mesmas palavras.

use std::collections::HashSet;

use rusqlite::Connection;

use crate::domain::normalize::{plain_words, Normalizer};
use crate::io::map_index::as_indexed;
use crate::platform::error::Result;

/// Uma palavra da pergunta: como está quebrada, as formas da normalização e
/// as formas como o índice as grava.
#[derive(Debug, Clone)]
pub(super) struct Word {
    pub plain: String,
    pub forms: Vec<String>,
    pub indexed: Vec<String>,
}

/// As palavras da pergunta, as mesmas que a busca dos arquivos usa: sem as de
/// ligação e sem repetir a palavra de mesmas formas.
pub(super) fn question(conn: &Connection, normalizer: &mut Normalizer, query: &str) -> Result<Vec<Word>> {
    let mut seen: HashSet<Vec<String>> = HashSet::new();
    let mut words: Vec<(String, Vec<String>)> = Vec::new();
    for word in plain_words(query) {
        if normalizer.is_function_word(&word) {
            continue;
        }
        let forms = normalizer.word_forms(&word);
        if seen.insert(forms.clone()) {
            words.push((word, forms));
        }
    }
    let forms: Vec<Vec<String>> = words.iter().map(|(_, forms)| forms.clone()).collect();
    let indexed = as_indexed(conn, &forms)?;
    Ok(words.into_iter().zip(indexed).map(|((plain, forms), indexed)| Word { plain, forms, indexed }).collect())
}
