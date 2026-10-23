//! `text` — o texto num lugar só: acento, minúsculas, palavras, as listas de
//! palavras comuns, a linha só e o corte com reticências.
//!
//! Antes, cada módulo tinha a sua cópia: duas tabelas de acento (o slug de
//! `platform::i18n` e a chave da busca em `scan_equivalences`), três listas
//! de palavras comuns (o slug, o voto de idioma da busca e a medição de
//! clareza) e cinco cortes de texto (o título do índice, a linha do resumo do
//! projeto, o título do commit, a opção citada pela testemunha e a linha da
//! mensagem sem destino). Agora todos moram aqui, e cada chamador usa estas
//! funções.
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

// ---------------------------------------------------------------------------
// Palavras comuns
// ---------------------------------------------------------------------------
//
// São listas diferentes porque as perguntas são diferentes. O voto de idioma
// da busca conta palavras funcionais dos dois lados, mesmo as que os dois
// idiomas têm ("a", "no"), porque só compara as contagens. A medição de clareza decide o idioma
// da resposta pelas palavras, então deixa de fora as que existem nos dois
// idiomas e aceita a grafia sem acento de quem digita sem ("nao", "voce").

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
// Linha e corte
// ---------------------------------------------------------------------------

/// O texto numa linha só, com um branco entre as palavras.
#[must_use]
pub fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Onde cai o corte do texto que passa do teto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutAt {
    /// Em qualquer caractere.
    Char,
    /// No fim da última palavra inteira que cabe; quando nem a primeira
    /// palavra cabe, em qualquer caractere.
    Word,
}

/// `text` com no máximo `max` caracteres, contadas as reticências. O texto que
/// cabe volta como está; o que passa perde o fim no ponto que `at` diz e
/// termina em `…`, sem branco nem pontuação solta antes delas.
///
/// É o único corte com reticências do projeto: o título, a frase do item, a
/// linha do painel e o trecho citado ao usuário cortam todos por aqui. Quem
/// quer o texto numa linha passa antes por [`one_line`].
#[must_use]
pub fn cut(text: &str, max: usize, at: CutAt) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let room = max.saturating_sub(1);
    let head = match at {
        CutAt::Word => Some(whole_words(text, room)).filter(|words| !words.is_empty()),
        CutAt::Char => None,
    }
    .unwrap_or_else(|| without_loose_end(first_chars(text, room)));
    format!("{head}…")
}

/// O começo de `text` que cabe em `room` caracteres, cortado no fim de uma
/// palavra inteira, sem branco nem pontuação solta no fim: vazio quando nem a
/// primeira palavra cabe. O texto que cabe volta como está. Não leva
/// reticências: serve a quem guarda o texto inteiro noutro lugar, como o
/// título do commit, que deixa o resumo inteiro no corpo.
#[must_use]
pub fn whole_words(text: &str, room: usize) -> &str {
    if text.chars().count() <= room {
        return text;
    }
    let kept = first_chars(text, room);
    let ends_a_word = text[kept.len()..].starts_with(char::is_whitespace);
    let head = if ends_a_word { kept } else { kept.rsplit_once(char::is_whitespace).map_or("", |(head, _)| head) };
    without_loose_end(head)
}

/// Os primeiros `count` caracteres de `text`.
fn first_chars(text: &str, count: usize) -> &str {
    text.char_indices().nth(count).map_or(text, |(at, _)| &text[..at])
}

/// `text` sem os brancos e a pontuação solta do fim.
fn without_loose_end(text: &str) -> &str {
    text.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-' | '—' | '.'))
}

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

        pub fn index_cut(text: &str, max: usize) -> String {
            if text.chars().count() <= max {
                return text.to_string();
            }
            let kept: String = text.chars().take(max.saturating_sub(1)).collect();
            format!("{}…", kept.trim_end())
        }

        pub fn summary_cut_at_word(text: &str, max: usize) -> String {
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.chars().count() <= max {
                return text;
            }
            let kept: String = text.chars().take(max.saturating_sub(1)).collect();
            let end = kept.rfind(' ').filter(|at| *at > 0).unwrap_or(kept.len());
            let head = kept[..end].trim_end_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-' | '—'));
            format!("{head}…")
        }

        pub fn commit_shorten_to(text: &str, room: usize) -> String {
            if text.chars().count() <= room {
                return text.to_string();
            }
            let kept: String = text.chars().take(room).collect();
            let whole_word = text.chars().nth(room).is_some_and(char::is_whitespace);
            let cut = if whole_word { kept.as_str() } else { kept.rsplit_once(char::is_whitespace).map_or("", |(head, _)| head) };
            cut.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-' | '.')).to_string()
        }

        /// A testemunha cortava sempre em 80; aqui o teto vem de fora.
        pub fn witness_truncate(s: &str, max: usize) -> String {
            if s.chars().count() <= max {
                return s.to_string();
            }
            let head: String = s.chars().take(max).collect();
            format!("{head}…")
        }

        /// A linha da mensagem sem destino cortava sempre em 100; aqui o teto
        /// vem de fora.
        pub fn unrouted_start(text: &str, max: usize) -> String {
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.chars().count() <= max {
                return text;
            }
            let start: String = text.chars().take(max).collect();
            format!("{}…", start.trim_end())
        }
    }

    /// Textos para a prova lado a lado: vazio, só ASCII e prosa com acento.
    const CORPUS: &[&str] = &[
        "",
        "git commit -m 'x'",
        "Conciliação do Título — ação já feita, você não viu?",
        "ÇÃÕ ÉÊ ÍÓÚ Ñ ñ",
        "Håkon ýÿ",
        "ação ação",
    ];

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

    #[test]
    fn words_split_on_anything_that_is_not_a_letter_or_digit() {
        let got: Vec<&str> = words("autorizar-pagamento (novo) ação_2").collect();
        assert_eq!(got, ["autorizar", "pagamento", "novo", "ação", "2"]);
        assert_eq!(words("  --  ").count(), 0);
    }

    /// O texto que cabe volta como está; o que passa fica no teto, contadas as
    /// reticências, e perde o branco e a pontuação solta de antes delas. O
    /// teto conta caracteres, não bytes.
    #[test]
    fn the_cut_keeps_the_ceiling_counting_the_ellipsis() {
        assert_eq!(cut("curto", 5, CutAt::Char), "curto");
        assert_eq!(cut("", 0, CutAt::Word), "");
        assert_eq!(cut("abcdef", 4, CutAt::Char), "abc…");
        assert_eq!(cut("ação ação", 6, CutAt::Char), "ação…");
        assert_eq!(cut("um, dois", 5, CutAt::Char), "um…");
        assert_eq!(cut("A fila, a ordem", 9, CutAt::Word), "A fila…");
    }

    /// O corte por palavra guarda a palavra que termina bem no teto, volta à
    /// última palavra inteira quando o teto cai no meio de uma, e corta em
    /// qualquer caractere a primeira palavra que sozinha passa do teto. Sem
    /// reticências, o começo em palavras inteiras fica vazio quando nem a
    /// primeira cabe.
    #[test]
    fn the_word_cut_ends_at_the_last_whole_word_that_fits() {
        let text = "Leitura das planilhas grandes sem travar";
        assert_eq!(cut(text, 30, CutAt::Word), "Leitura das planilhas grandes…");
        assert_eq!(cut(text, 29, CutAt::Word), "Leitura das planilhas…");
        assert_eq!(cut("planilhasgrandes demais", 8, CutAt::Word), "planilh…");
        assert_eq!(whole_words(text, 29), "Leitura das planilhas grandes");
        assert_eq!(whole_words(text, 6), "");
        assert_eq!(whole_words("curto.", 6), "curto.");
        assert_eq!(whole_words("Fim — e mais", 6), "Fim");
    }

    /// Textos para a prova lado a lado dos cortes: vazio, curto, prosa com
    /// pontuação e acento, uma palavra maior que muitos tetos e quebras.
    const CUT_CORPUS: &[&str] = &[
        "",
        "curto",
        "Leitura das planilhas grandes sem travar",
        "A fila, a ordem; o aviso: tudo — e mais. Fim.",
        "Conciliação do Título — ação já feita, você não viu?",
        "umapalavrasemfimnenhumaqueseestende e outra",
        "Linha um\nlinha  dois\tcom tab",
        "  começa com branco",
    ];

    /// O corte único responde como os cinco que ele substitui, em todo teto,
    /// com três diferenças de propósito: a pontuação solta sai de antes das
    /// reticências; o corte por palavra guarda a palavra que termina bem no
    /// teto, que o resumo do projeto jogava fora; e o texto que passa do teto
    /// por um caractere só, na opção citada e na mensagem sem destino, sai
    /// inteiro em vez de trocar o último caractere pelas reticências.
    #[test]
    fn the_single_cut_answers_like_the_five_it_replaces() {
        let loose = |old: String| old.strip_suffix('…').map_or_else(|| old.clone(), |head| format!("{}…", without_loose_end(head)));
        for text in CUT_CORPUS {
            let line = one_line(text);
            for max in 0..=text.chars().count() + 2 {
                let at = format!("{text:?} em {max}");
                let index = if text.chars().count() <= max { (*text).to_string() } else { loose(before::index_cut(text, max)) };
                assert_eq!(cut(text, max, CutAt::Char), index, "índice: {at}");

                let room = max.saturating_sub(1);
                let summary = if line.chars().count() <= max {
                    line.clone()
                } else if line.chars().nth(room) == Some(' ') {
                    format!("{}…", without_loose_end(first_chars(&line, room)))
                } else {
                    loose(before::summary_cut_at_word(text, max))
                };
                assert_eq!(cut(&line, max, CutAt::Word), summary, "resumo: {at}");

                let commit = before::commit_shorten_to(text, max);
                let commit = if text.chars().count() <= max { commit } else { without_loose_end(&commit).to_string() };
                assert_eq!(whole_words(text, max), commit, "commit: {at}");

                let witness = if text.chars().count() <= max + 1 { (*text).to_string() } else { loose(before::witness_truncate(text, max)) };
                assert_eq!(cut(text, max + 1, CutAt::Char), witness, "testemunha: {at}");

                let unrouted = if line.chars().count() <= max + 1 { line.clone() } else { loose(before::unrouted_start(text, max)) };
                assert_eq!(cut(&line, max + 1, CutAt::Char), unrouted, "sem destino: {at}");
            }
        }
    }
}
