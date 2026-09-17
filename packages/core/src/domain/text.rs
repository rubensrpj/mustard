//! `text` — o texto num lugar só: acento, minúsculas, palavras, "tem a
//! palavra" e as listas de palavras comuns.
//!
//! Antes, cada módulo tinha a sua cópia: duas tabelas de acento (o slug de
//! `platform::i18n` e a chave da busca em `scan_equivalences`), três listas
//! de palavras comuns (o slug, o voto de idioma da busca e a medição de
//! clareza) e seis variações de "tem a palavra" (a trava de comandos, o
//! portão de tamanho, a varredura de dívida do fechamento e a conferência de
//! dependências). Agora todas moram aqui, e cada chamador usa estas funções.
//!
//! Função pura: sem disco, sem log, sem relógio.

// ---------------------------------------------------------------------------
// Acento e minúsculas
// ---------------------------------------------------------------------------

/// Troca cada letra acentuada pela letra sem acento, mantendo maiúscula e
/// minúscula: `"Ação"` vira `"Acao"`.
///
/// Cobre as letras do português e as poucas vizinhas que a tabela da busca já
/// cobria (`å`, `ý`, `ÿ`). Não é um normalizador Unicode completo: trazer
/// `unicode-normalization` para o núcleo só por isso não compensa.
#[must_use]
pub fn fold_accents(input: &str) -> String {
    input.chars().map(fold_char).collect()
}

/// Minúsculas e sem acento: a forma em que dois textos se comparam.
/// `"Conciliação"` e `"CONCILIACAO"` viram `"conciliacao"`.
#[must_use]
pub fn fold(input: &str) -> String {
    fold_accents(&input.to_lowercase())
}

fn fold_char(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'A',
        'ç' => 'c',
        'Ç' => 'C',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'É' | 'È' | 'Ê' | 'Ë' => 'E',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
        'ñ' => 'n',
        'Ñ' => 'N',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Palavras
// ---------------------------------------------------------------------------

/// As palavras de um texto: os pedaços entre caracteres que não são letra nem
/// dígito, sem os vazios. `"autorizar-pagamento (novo)"` dá `autorizar`,
/// `pagamento` e `novo`.
pub fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty())
}

/// `true` para um byte de palavra ASCII: letra, dígito ou `_` (o `\w` das
/// expressões regulares).
#[must_use]
pub fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Que caractere conta como parte de uma palavra na hora de conferir a borda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordChars {
    /// Letra ou dígito ASCII. A trava de comandos usa esta: `git_push` tem
    /// borda antes de `push`.
    Alphanumeric,
    /// Letra, dígito ASCII ou `_` — o `\w` das expressões regulares.
    AlphanumericOrUnderscore,
}

impl WordChars {
    fn contains(self, b: u8) -> bool {
        match self {
            Self::Alphanumeric => b.is_ascii_alphanumeric(),
            Self::AlphanumericOrUnderscore => is_word_byte(b),
        }
    }
}

/// As bordas que [`has_word_sequence`] confere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boundaries {
    /// Que caractere conta como parte de uma palavra.
    pub word_chars: WordChars,
    /// Antes da primeira palavra: começo do texto ou um caractere que não é
    /// de palavra.
    pub left: bool,
    /// Depois da última palavra: fim do texto ou um caractere que não é de
    /// palavra.
    pub right: bool,
}

impl Boundaries {
    /// `\bpalavra\b`: as duas bordas, com `_` como letra — o `\b` das
    /// expressões regulares.
    pub const WHOLE: Self =
        Self { word_chars: WordChars::AlphanumericOrUnderscore, left: true, right: true };
}

/// `true` quando `text` tem `words` em sequência, separadas por ao menos um
/// espaço, com as bordas pedidas — o `\bA\s+B\b` das expressões regulares,
/// sem regex. `has_word_sequence("rtk git  push", &["git", "push"],
/// Boundaries::WHOLE)` é `true`; `"gitpush"` não é.
///
/// A comparação é exata: quem quer ignorar maiúsculas passa o texto já em
/// minúsculas. Sem palavras, a resposta é `false`.
#[must_use]
pub fn has_word_sequence(text: &str, words: &[&str], bounds: Boundaries) -> bool {
    let Some((first, rest)) = words.split_first() else {
        return false;
    };
    let bytes = text.as_bytes();
    let mut from = 0;
    while from <= text.len() {
        let Some(rel) = text[from..].find(first) else {
            break;
        };
        let start = from + rel;
        let end = start + first.len();
        let left_ok = !bounds.left || start == 0 || !bounds.word_chars.contains(bytes[start - 1]);
        if left_ok && tail_matches(&text[end..], rest, bounds) {
            return true;
        }
        // Palavra vazia casa em todo lugar sem avançar: pula um caractere.
        from = if end > start {
            end
        } else {
            text[start..].chars().next().map_or(text.len() + 1, |c| start + c.len_utf8())
        };
    }
    false
}

/// O resto da sequência a partir do fim da primeira palavra: cada palavra
/// seguinte depois de ao menos um espaço, e a borda direita no fim.
fn tail_matches(mut rest: &str, words: &[&str], bounds: Boundaries) -> bool {
    for word in words {
        let trimmed = rest.trim_start();
        if trimmed.len() == rest.len() || !trimmed.starts_with(word) {
            return false;
        }
        rest = &trimmed[word.len()..];
    }
    !bounds.right || rest.as_bytes().first().is_none_or(|&b| !bounds.word_chars.contains(b))
}

// ---------------------------------------------------------------------------
// Palavras comuns
// ---------------------------------------------------------------------------
//
// São três listas porque as três perguntas são diferentes. O slug descarta só
// o que encurta um nome sem perder o sentido. O voto de idioma da busca conta
// palavras funcionais dos dois lados, mesmo as que os dois idiomas têm ("a",
// "no"), porque só compara as contagens. A medição de clareza decide o idioma
// da resposta pelas palavras, então deixa de fora as que existem nos dois
// idiomas e aceita a grafia sem acento de quem digita sem ("nao", "voce").

/// Artigos e preposições que o slug do português descarta.
pub const SLUG_STOPWORDS_PT: &[&str] = &[
    "a", "o", "as", "os", "de", "da", "do", "das", "dos", "e", "em",
    // Contrações de `em`/`a` com artigo: sem elas, um `no` no fim ("em o")
    // ocupa uma vaga do slug e empurra a palavra seguinte para fora.
    "no", "na", "nos", "nas", "ao", "aos",
];

/// Artigos e preposições que o slug do inglês descarta.
pub const SLUG_STOPWORDS_EN: &[&str] = &["a", "an", "the", "of", "and", "or", "in"];

/// Palavras funcionais do inglês para o voto de idioma da busca.
pub const FUNCTION_WORDS_EN: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "of", "at", "by", "for", "with", "about", "into",
    "through", "before", "after", "to", "from", "in", "out", "on", "off", "over", "under", "again",
    "then", "once", "here", "there", "when", "where", "why", "how", "all", "any", "both", "each",
    "few", "more", "most", "some", "such", "no", "not", "only", "same", "than", "too", "very",
    "is", "are", "was", "were", "been", "being", "be", "have", "has", "had", "does", "did", "this",
    "that", "these", "those", "will", "would", "can", "could", "should", "must", "it", "its",
    "his", "her", "our", "their", "your", "you", "they", "she", "what", "which", "who", "as",
];

/// Palavras funcionais do português para o voto de idioma da busca.
pub const FUNCTION_WORDS_PT: &[&str] = &[
    "o", "a", "os", "as", "um", "uma", "uns", "umas", "de", "do", "da", "dos", "das", "no", "na",
    "nos", "nas", "ao", "aos", "à", "às", "pelo", "pela", "pelos", "pelas", "em", "por", "para",
    "com", "sem", "sob", "sobre", "entre", "até", "e", "ou", "mas", "que", "se", "não", "sim",
    "é", "são", "foi", "foram", "ser", "sendo", "era", "eram", "está", "estão", "estava", "tem",
    "têm", "tinha", "há", "já", "mais", "menos", "muito", "muitos", "como", "quando", "onde",
    "qual", "quais", "quem", "isso", "isto", "esse", "essa", "esses", "essas", "este", "esta",
    "estes", "estas", "ele", "ela", "eles", "elas", "você", "nós", "eu", "seu", "sua", "seus",
    "suas", "meu", "minha", "nosso", "nossa", "também", "depois", "antes", "agora", "aqui",
    "cada", "todo", "toda", "todos", "todas", "outro", "outra", "outros", "outras", "mesmo",
    "mesma", "ainda", "então", "pois", "porque",
];

/// Palavras comuns do português para a medição de clareza, com e sem acento.
/// Ficam de fora as que existem nos dois idiomas ("a", "as", "no", "do", "se",
/// "for") e as que o inglês usa sozinhas ("todo", "ate", "ha").
pub const COMMON_WORDS_PT: &[&str] = &[
    "o", "os", "um", "uma", "uns", "umas", "de", "da", "das", "dos", "na", "nas", "nos", "em",
    "ao", "aos", "à", "às", "pelo", "pela", "pelos", "pelas", "para", "por", "com", "sem",
    "sobre", "até", "e", "ou", "mas", "que", "não", "nao", "é", "são", "sao", "foi", "foram",
    "ser", "está", "estão", "estao", "tem", "têm", "há", "já", "mais", "muito", "como", "quando",
    "onde", "qual", "isso", "isto", "esse", "essa", "este", "esta", "ele", "ela", "eles", "elas",
    "você", "voce", "seu", "sua", "também", "tambem", "depois", "agora", "aqui", "cada", "toda",
    "outro", "outra", "mesmo", "ainda", "então", "entao", "pois", "porque",
];

/// Palavras comuns do inglês para a medição de clareza, nenhuma delas palavra
/// do português.
pub const COMMON_WORDS_EN: &[&str] = &[
    "the", "and", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had", "does",
    "did", "of", "to", "in", "on", "at", "by", "with", "from", "into", "about", "this", "that",
    "these", "those", "it", "its", "not", "but", "or", "if", "then", "than", "there", "their",
    "they", "we", "you", "your", "our", "he", "she", "will", "would", "can", "could", "should",
    "which", "what", "who", "when", "where", "how", "why", "an", "all", "any", "each", "only",
    "also", "now", "here", "just", "after", "before",
];

// ---------------------------------------------------------------------------
// Linhas de código
// ---------------------------------------------------------------------------

/// O teto de linhas de código de um arquivo de produção que fica.
pub const CODE_LINE_CAP: usize = 800;

/// As linhas de código de um arquivo Rust: as que não são vazias nem
/// comentário, antes do módulo de testes dele. É a medida do teto de
/// [`CODE_LINE_CAP`].
#[must_use]
pub fn code_lines(source: &str) -> usize {
    let lines: Vec<&str> = source.lines().map(str::trim).collect();
    let end = lines
        .windows(2)
        .position(|pair| pair[0] == "#[cfg(test)]" && pair[1] == "mod tests {")
        .unwrap_or(lines.len());
    lines[..end].iter().filter(|line| !line.is_empty() && !line.starts_with("//")).count()
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// As funções que este módulo substitui, copiadas como eram, para o teste
    /// lado a lado provar que a troca não mudou resposta nenhuma.
    mod before {
        pub fn i18n_strip_pt_accents(input: &str) -> String {
            let mut out = String::with_capacity(input.len());
            for ch in input.chars() {
                let replacement = match ch {
                    'ç' => 'c',
                    'Ç' => 'C',
                    'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
                    'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'A',
                    'é' | 'è' | 'ê' | 'ë' => 'e',
                    'É' | 'È' | 'Ê' | 'Ë' => 'E',
                    'í' | 'ì' | 'î' | 'ï' => 'i',
                    'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
                    'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
                    'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
                    'ú' | 'ù' | 'û' | 'ü' => 'u',
                    'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
                    'ñ' => 'n',
                    'Ñ' => 'N',
                    other => other,
                };
                out.push(replacement);
            }
            out
        }

        pub fn scan_fold_tok(s: &str) -> String {
            s.to_lowercase()
                .chars()
                .map(|c| match c {
                    'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
                    'ç' => 'c',
                    'è' | 'é' | 'ê' | 'ë' => 'e',
                    'ì' | 'í' | 'î' | 'ï' => 'i',
                    'ñ' => 'n',
                    'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
                    'ù' | 'ú' | 'û' | 'ü' => 'u',
                    'ý' | 'ÿ' => 'y',
                    _ => c,
                })
                .collect()
        }

        fn is_word_byte(b: u8) -> bool {
            b.is_ascii_alphanumeric() || b == b'_'
        }

        pub fn lex_has_word_pair(cmd: &str, a: &str, b: &str) -> bool {
            let mut search_from = 0;
            while let Some(rel) = cmd[search_from..].find(a) {
                let a_start = search_from + rel;
                let a_end = a_start + a.len();
                let left_ok = a_start == 0 || !cmd.as_bytes()[a_start - 1].is_ascii_alphanumeric();
                let rest = &cmd[a_end..];
                let trimmed = rest.trim_start();
                let had_ws = trimmed.len() < rest.len();
                if left_ok && had_ws && trimmed.starts_with(b) {
                    let b_end_byte = trimmed.as_bytes().get(b.len());
                    let right_ok = b_end_byte.is_none_or(|c| !c.is_ascii_alphanumeric());
                    if right_ok {
                        return true;
                    }
                }
                search_from = a_end;
            }
            false
        }

        pub fn lex_has_word(cmd: &str, needle: &str) -> bool {
            let mut from = 0;
            while let Some(rel) = cmd[from..].find(needle) {
                let start = from + rel;
                let left_ok = start == 0 || !cmd.as_bytes()[start - 1].is_ascii_alphanumeric();
                if left_ok {
                    return true;
                }
                from = start + needle.len();
            }
            false
        }

        pub fn size_gate_has_word_pair_loose(s: &str, a: &str, b: &str) -> bool {
            let mut from = 0;
            while let Some(rel) = s[from..].find(a) {
                let start = from + rel;
                let end = start + a.len();
                let left_ok = start == 0 || !is_word_byte(s.as_bytes()[start - 1]);
                let rest = &s[end..];
                let trimmed = rest.trim_start();
                let had_ws = trimmed.len() < rest.len();
                if left_ok && had_ws && trimmed.starts_with(b) {
                    return true;
                }
                from = end;
            }
            false
        }

        pub fn size_gate_contains_word(s: &str, word: &str) -> bool {
            let mut from = 0;
            while let Some(rel) = s[from..].find(word) {
                let start = from + rel;
                let end = start + word.len();
                let left_ok = start == 0 || !is_word_byte(s.as_bytes()[start - 1]);
                let right_ok = s.as_bytes().get(end).is_none_or(|&b| !is_word_byte(b));
                if left_ok && right_ok {
                    return true;
                }
                from = end;
            }
            false
        }

        pub fn close_gates_has_word_pair(s: &str, a: &str, b: &str) -> bool {
            let mut from = 0;
            while let Some(rel) = s[from..].find(a) {
                let start = from + rel;
                let end = start + a.len();
                let left_ok = start == 0 || !is_word_byte(s.as_bytes()[start - 1]);
                let rest = &s[end..];
                let trimmed = rest.trim_start();
                let had_ws = trimmed.len() < rest.len();
                if left_ok
                    && had_ws
                    && trimmed.starts_with(b)
                    && trimmed.as_bytes().get(b.len()).is_none_or(|&c| !is_word_byte(c))
                {
                    return true;
                }
                from = end;
            }
            false
        }

        pub fn close_gates_has_word_triple(s: &str, a: &str, b: &str, c: &str) -> bool {
            let mut from = 0;
            while let Some(rel) = s[from..].find(a) {
                let start = from + rel;
                let end = start + a.len();
                let left_ok = start == 0 || !is_word_byte(s.as_bytes()[start - 1]);
                if left_ok {
                    let rest = &s[end..];
                    let after_a = rest.trim_start();
                    if after_a.len() < rest.len() && after_a.starts_with(b) {
                        let after_b = &after_a[b.len()..];
                        let after_b_trim = after_b.trim_start();
                        if after_b_trim.len() < after_b.len()
                            && after_b_trim.starts_with(c)
                            && after_b_trim.as_bytes().get(c.len()).is_none_or(|&x| !is_word_byte(x))
                        {
                            return true;
                        }
                    }
                }
                from = end;
            }
            false
        }

        pub fn dependency_has_word_boundary_hit(content: &str, needle: &str, symbol: &str) -> bool {
            let mut from = 0usize;
            while let Some(idx) = content[from..].find(needle) {
                let absolute = from + idx;
                let end = absolute + needle.len();
                if needle.ends_with(symbol) {
                    let next = content.as_bytes().get(end).copied();
                    let is_word = matches!(next, Some(b) if b.is_ascii_alphanumeric() || b == b'_');
                    if !is_word {
                        return true;
                    }
                } else {
                    return true;
                }
                from = end;
            }
            false
        }

        pub const SLUG_PT: &[&str] = &[
            "a", "o", "as", "os", "de", "da", "do", "das", "dos", "e", "em", "no", "na", "nos",
            "nas", "ao", "aos",
        ];
        pub const SLUG_EN: &[&str] = &["a", "an", "the", "of", "and", "or", "in"];
    }

    /// Textos variados: comandos com e sem `rtk`, critérios de aceite, prosa
    /// com acento, bordas com `_`, tabulação e quebra de linha entre palavras,
    /// palavra repetida e texto vazio.
    const CORPUS: &[&str] = &[
        "",
        "git commit -m 'x'",
        "rtk git   push origin -f",
        "git_push --force",
        "xgit push",
        "git\tpush\n",
        "git pushy",
        "git push_x",
        "echo git && git push",
        "git git push",
        "sudo shutdown -h now",
        "preshutdown",
        "my_shutdown",
        "mkfs.ext4 /dev/sda",
        "dd if=/dev/zero of=x",
        "chmod 777 file",
        "chmod 7777 file",
        "command: node -e \"x\"",
        "command: node -eval",
        "command: bash -c 'grep x'",
        "command: cat a | jq .",
        "command: sqlite3 db",
        "command: grep_x file",
        "not yet implemented",
        "not  yet\timplemented_x",
        "cannot yet implemented",
        "this is not yet implementedness",
        "future hook here",
        "the future  hook",
        "futurehook",
        "export const FooBar = 1",
        "export const Foo = 1",
        "export const Foo, Bar",
        "import { Foo_ } from 'x'",
        "Conciliação do Título — ação já feita, você não viu?",
        "ÇÃÕ ÉÊ ÍÓÚ Ñ ñ",
        "Håkon ýÿ",
        "ação ação",
    ];

    const PAIRS: &[(&str, &str)] = &[
        ("git", "commit"),
        ("git", "push"),
        ("chmod", "777"),
        ("dd", "if="),
        ("node", "-e"),
        ("bash", "-c"),
        ("future", "hook"),
        ("ação", "ação"),
    ];

    const SINGLES: &[&str] =
        &["shutdown", "mkfs", "grep", "jq", "sqlite3", "cat", "foo", "Foo", "Foo_", "ação", "push"];

    fn at(word_chars: WordChars, left: bool, right: bool) -> Boundaries {
        Boundaries { word_chars, left, right }
    }

    /// Critério da onda de preparo: as funções antigas de acento e o `text`
    /// dão a mesma resposta para a mesma entrada. A única diferença é de
    /// propósito: a tabela única também tira o acento de `å`, `ý` e `ÿ` no
    /// slug, que a tabela do slug não cobria.
    #[test]
    fn accents_answer_like_the_two_old_tables() {
        for text in CORPUS {
            assert_eq!(fold(text), before::scan_fold_tok(text), "fold_tok: {text:?}");
            let only_old_table: String =
                text.chars().filter(|c| !matches!(c, 'å' | 'ý' | 'ÿ')).collect();
            assert_eq!(
                fold_accents(&only_old_table),
                before::i18n_strip_pt_accents(&only_old_table),
                "strip_pt_accents: {text:?}"
            );
        }
        assert_eq!(fold_accents("Håkon ýÿ"), "Hakon yy");
    }

    /// Critério da onda de preparo: cada variação antiga de "tem a palavra"
    /// dá a mesma resposta que [`has_word_sequence`] com as bordas dela.
    #[test]
    fn has_word_answers_like_every_old_variation() {
        let shell = at(WordChars::Alphanumeric, true, true);
        let shell_prefix = at(WordChars::Alphanumeric, true, false);
        let loose = at(WordChars::AlphanumericOrUnderscore, true, false);
        let right_only = at(WordChars::AlphanumericOrUnderscore, false, true);
        for text in CORPUS {
            for (a, b) in PAIRS {
                let pair = [*a, *b];
                assert_eq!(
                    has_word_sequence(text, &pair, shell),
                    before::lex_has_word_pair(text, a, b),
                    "lex pair {a} {b} in {text:?}"
                );
                assert_eq!(
                    has_word_sequence(text, &pair, loose),
                    before::size_gate_has_word_pair_loose(text, a, b),
                    "loose pair {a} {b} in {text:?}"
                );
                assert_eq!(
                    has_word_sequence(text, &pair, Boundaries::WHOLE),
                    before::close_gates_has_word_pair(text, a, b),
                    "close pair {a} {b} in {text:?}"
                );
            }
            for word in SINGLES {
                assert_eq!(
                    has_word_sequence(text, &[word], shell_prefix),
                    before::lex_has_word(text, word),
                    "lex word {word} in {text:?}"
                );
                assert_eq!(
                    has_word_sequence(text, &[word], Boundaries::WHOLE),
                    before::size_gate_contains_word(text, word),
                    "contains_word {word} in {text:?}"
                );
                for symbol in ["Foo", "x"] {
                    let new = if word.ends_with(symbol) {
                        has_word_sequence(text, &[word], right_only)
                    } else {
                        text.contains(word)
                    };
                    assert_eq!(
                        new,
                        before::dependency_has_word_boundary_hit(text, word, symbol),
                        "boundary hit {word}/{symbol} in {text:?}"
                    );
                }
            }
            assert_eq!(
                has_word_sequence(text, &["not", "yet", "implemented"], Boundaries::WHOLE),
                before::close_gates_has_word_triple(text, "not", "yet", "implemented"),
                "triple in {text:?}"
            );
        }
    }

    /// Critério da onda de preparo: as listas do slug chegaram iguais.
    #[test]
    fn slug_stopwords_are_the_old_lists() {
        assert_eq!(SLUG_STOPWORDS_PT, before::SLUG_PT);
        assert_eq!(SLUG_STOPWORDS_EN, before::SLUG_EN);
    }

    #[test]
    fn words_split_on_anything_that_is_not_a_letter_or_digit() {
        let got: Vec<&str> = words("autorizar-pagamento (novo) ação_2").collect();
        assert_eq!(got, ["autorizar", "pagamento", "novo", "ação", "2"]);
        assert_eq!(words("  --  ").count(), 0);
    }

    #[test]
    fn has_word_sequence_edges() {
        assert!(!has_word_sequence("git push", &[], Boundaries::WHOLE), "no words, no match");
        // Palavra vazia não trava o laço: casa onde as bordas deixam.
        let left_only = Boundaries { right: false, ..Boundaries::WHOLE };
        assert!(has_word_sequence("abc", &[""], left_only), "at the start of the text");
        assert!(!has_word_sequence("abc", &[""], Boundaries::WHOLE), "no boundary on both sides");
        assert!(has_word_sequence("é git push", &["git", "push"], Boundaries::WHOLE));
        assert!(!has_word_sequence("gitpush", &["git", "push"], Boundaries::WHOLE));
    }
}
