//! As palavras que a triagem do mapa lê no padrão de uma busca.
//!
//! O padrão é lido por alternativas: `A\|B` e `A|B` são duas opções, cada uma
//! com as suas palavras, e não uma frase só. De cada alternativa saem os
//! trechos de letras, números e sublinhado com ao menos dois caracteres, sem
//! os operadores da expressão (`\w`, `\(`, `[a-z]`, `{2,3}`), na ordem e sem
//! repetir. O nome composto entra inteiro, além das partes: `pr_open` traz
//! `pr` e `open`, e `use-auth` ou `user.service`, escritos com hífen ou com
//! ponto, trazem também `use_auth` e `user_service`, o nome como o código o
//! declara. Quando o teto de palavras corta, cada alternativa fica com as
//! suas primeiras, em rodada, e nenhuma some por estar no fim do padrão.

/// Quantas palavras da busca vão à triagem.
pub(crate) const MAX_WORDS: usize = 12;

/// As extensões de arquivo: a que fecha um nome composto (`user.service.ts`)
/// fica fora do nome inteiro, que o código declara sem ela.
const EXTENSIONS: [&str; 24] = [
    "json", "yaml", "yml", "toml", "md", "ts", "tsx", "js", "jsx", "mjs", "rs", "py", "cs", "go", "java", "css", "scss", "html",
    "sql", "txt", "lock", "xml", "csv", "sh",
];

/// O que o padrão tem, na ordem: um trecho de nome, o elo entre dois trechos
/// (`-` e `.`), o corte que separa palavras ou a passagem para a alternativa
/// seguinte.
enum Piece {
    Word(String),
    Join,
    Cut,
    Next,
}

/// As palavras do padrão de cada uma das `patterns`, a de cada alternativa à
/// frente da alternativa seguinte. Em texto fixo (`fixed`), `|` e `\\` são
/// letras do texto: não há operador nem alternativa.
pub(crate) fn words_of(patterns: &[String], fixed: bool) -> Vec<String> {
    let mut alternatives: Vec<Vec<String>> = Vec::new();
    for pattern in patterns {
        for alternative in split(&pieces(pattern, fixed)) {
            alternatives.push(words_in(&alternative));
        }
    }
    let mut natural: Vec<String> = Vec::new();
    for word in alternatives.iter().flatten() {
        if !natural.contains(word) {
            natural.push(word.clone());
        }
    }
    if natural.len() <= MAX_WORDS {
        return natural;
    }
    let mut kept: Vec<usize> = Vec::new();
    let mut round = 0;
    while kept.len() < MAX_WORDS {
        let mut any = false;
        for words in &alternatives {
            let Some(word) = words.get(round) else { continue };
            any = true;
            let Some(place) = natural.iter().position(|seen| seen == word) else { continue };
            if !kept.contains(&place) && kept.len() < MAX_WORDS {
                kept.push(place);
            }
        }
        if !any {
            break;
        }
        round += 1;
    }
    kept.sort_unstable();
    kept.into_iter().map(|place| natural[place].clone()).collect()
}

/// O padrão em trechos, cortes e passagens de alternativa.
fn pieces(pattern: &str, fixed: bool) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut word = String::new();
    let mut chars = pattern.chars();
    let flush = |word: &mut String, out: &mut Vec<Piece>| {
        if !word.is_empty() {
            out.push(Piece::Word(std::mem::take(word)));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\\' if !fixed => {
                let escaped = chars.next();
                flush(&mut word, &mut out);
                out.push(match escaped {
                    Some('|') => Piece::Next,
                    Some('.') => Piece::Join,
                    _ => Piece::Cut,
                });
            }
            '[' if !fixed => {
                flush(&mut word, &mut out);
                skip_class(&mut chars);
                out.push(Piece::Cut);
            }
            '{' if !fixed => {
                flush(&mut word, &mut out);
                for skipped in chars.by_ref() {
                    if skipped == '}' {
                        break;
                    }
                }
                out.push(Piece::Cut);
            }
            '|' if !fixed => {
                flush(&mut word, &mut out);
                out.push(Piece::Next);
            }
            '.' if !fixed && matches!(chars.clone().next(), Some('*' | '+' | '?')) => {
                flush(&mut word, &mut out);
                out.push(Piece::Cut);
            }
            '.' | '-' => {
                flush(&mut word, &mut out);
                out.push(Piece::Join);
            }
            c if c.is_alphanumeric() || c == '_' => word.push(c),
            _ => {
                flush(&mut word, &mut out);
                out.push(Piece::Cut);
            }
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Os trechos de `pieces` entre duas passagens de alternativa.
fn split(pieces: &[Piece]) -> Vec<Vec<&Piece>> {
    let mut out: Vec<Vec<&Piece>> = vec![Vec::new()];
    for piece in pieces {
        if matches!(piece, Piece::Next) {
            out.push(Vec::new());
        } else if let Some(last) = out.last_mut() {
            last.push(piece);
        }
    }
    out
}

/// As palavras de uma alternativa, na ordem e sem repetir: o nome composto
/// (trechos ligados por `-` ou `.`) vem inteiro, com `_` no lugar do elo, e
/// depois as partes dele.
fn words_in(pieces: &[&Piece]) -> Vec<String> {
    let mut chains: Vec<Vec<&str>> = Vec::new();
    let mut joined = false;
    for piece in pieces {
        match piece {
            Piece::Word(word) => {
                match chains.last_mut() {
                    Some(chain) if joined => chain.push(word),
                    _ => chains.push(vec![word]),
                }
                joined = false;
            }
            Piece::Join => joined = !chains.is_empty() && !joined,
            Piece::Cut | Piece::Next => joined = false,
        }
    }
    let mut words: Vec<String> = Vec::new();
    let mut add = |word: &str| {
        if word.chars().count() >= 2 && !words.iter().any(|seen| seen == word) {
            words.push(word.to_string());
        }
    };
    for chain in chains {
        let ends_in_extension = chain.len() > 1 && chain.last().is_some_and(|last| EXTENSIONS.contains(&last.to_lowercase().as_str()));
        let name = if ends_in_extension { &chain[..chain.len() - 1] } else { &chain[..] };
        if name.len() >= 2 && name.iter().all(|part| part.chars().count() >= 2) {
            add(&name.join("_"));
        }
        for part in chain {
            add(part);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    fn words(pattern: &str) -> Vec<String> {
        words_of(&owned(&[pattern]), false)
    }

    /// O nome composto entra inteiro, como o código o declara, e as partes
    /// dele vêm em seguida: com hífen ou com ponto (`\.` ou `.`), o elo vira
    /// `_`; o `.` de `.*` é curinga, e não elo.
    #[test]
    fn a_composed_name_comes_whole_and_then_in_parts() {
        assert_eq!(words("pr_open"), ["pr_open"]);
        assert_eq!(words("use-auth"), ["use_auth", "use", "auth"]);
        assert_eq!(words(r"user\.service"), ["user_service", "user", "service"]);
        assert_eq!(words("user.service"), ["user_service", "user", "service"]);
        assert_eq!(words("foo.*bar"), ["foo", "bar"]);
        assert_eq!(words("a.b"), Vec::<String>::new(), "one letter is no part of a name");
        assert_eq!(words(r"land\.json"), ["land", "json"], "the extension joins no name");
        assert_eq!(words("user.service.ts"), ["user_service", "user", "service", "ts"], "the extension is left out of the whole name");
        assert_eq!(words("--verbose"), ["verbose"], "a dash with nothing before it joins nothing");
        assert_eq!(words("pr-open|pr-open"), ["pr_open", "pr", "open"], "a repeat counts once");
    }

    /// `A\|B` e `A|B` são duas alternativas, cada uma com as suas palavras;
    /// em texto fixo, `|` é letra do texto e não há alternativa.
    #[test]
    fn a_choice_is_read_as_alternatives_each_with_its_words() {
        assert_eq!(words(r"pr_open\|PHASES"), ["pr_open", "PHASES"]);
        assert_eq!(words("pr_open|PHASES"), ["pr_open", "PHASES"]);
        assert_eq!(words(r"use-auth\|log_in"), ["use_auth", "use", "auth", "log_in"]);
        assert_eq!(words_of(&owned(&["pr_open|PHASES"]), true), ["pr_open", "PHASES"], "in fixed text the bar only cuts");
        assert_eq!(words(r"(foo|bar)baz"), ["foo", "bar", "baz"]);
    }

    /// Com mais palavras que o teto, cada alternativa fica com as suas
    /// primeiras, em rodada: a última não some por estar no fim do padrão.
    #[test]
    fn a_cut_keeps_the_first_words_of_each_alternative_and_not_only_the_first_alternative() {
        let long: Vec<String> = (0..14).map(|n| format!("first{n}")).collect();
        for separator in [r"\|", "|"] {
            let pattern = format!("{}{separator}zeta{separator}omega", long.join(" "));
            let kept = words(&pattern);
            assert_eq!(kept.len(), MAX_WORDS, "{kept:?}");
            assert!(kept.contains(&"zeta".to_string()) && kept.contains(&"omega".to_string()), "{kept:?}");
            assert_eq!(&kept[..3], ["first0", "first1", "first2"], "the natural order stays: {kept:?}");
        }
    }
}
