//! A regra das palavras em maiúsculas nas frases do programa.
//!
//! Só os testes a usam: eles conferem o catálogo de frases e a ajuda dos
//! comandos de `mustard`, `mustard-rt` e `scan`. Cada pacote traz este arquivo
//! pelo caminho, como o de `manifest_dir.rs`, para a regra ter um texto só e
//! não ir no programa instalado.

/// As palavras em maiúsculas que podem ficar fora de crase numa frase do
/// programa: siglas e unidades de uso comum. Uma sigla nova só passa se
/// alguém a puser aqui de propósito.
pub const CAPS_ALLOWED: [&str; 4] = ["JSON", "UTF", "MB", "GB"];

/// Cada palavra de duas letras ou mais, toda em maiúsculas e fora de crase,
/// uma vez só, na ordem em que aparece, salvo as de [`CAPS_ALLOWED`].
///
/// É a regra única das frases fixas do programa, as do catálogo e as da
/// ajuda dos comandos: elas são escritas por nós, então a conferência não
/// adivinha se a palavra é grito ou sigla, e o nome de código vai entre
/// crases. Lista vazia quer dizer que o texto segue a regra.
pub fn uppercase_words(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for outside in text.split('`').step_by(2) {
        for word in outside.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            let letters = word.chars().filter(|c| c.is_alphabetic()).count();
            let upper = letters >= 2 && !word.chars().any(char::is_lowercase);
            if upper && !CAPS_ALLOWED.contains(&word) && !found.contains(&word) {
                found.push(word);
            }
        }
    }
    found
}
