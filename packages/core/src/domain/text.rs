//! `text` — o texto num lugar só: acento, minúsculas, palavras e as listas de
//! palavras comuns.
//!
//! Antes, cada módulo tinha a sua cópia: duas tabelas de acento (o slug de
//! `platform::i18n` e a chave da busca em `scan_equivalences`) e três listas
//! de palavras comuns (o slug, o voto de idioma da busca e a medição de
//! clareza). Agora todas moram aqui, e cada chamador usa estas funções.
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
}
