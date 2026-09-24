//! `mustard_id` — o identificador único de um item do Mustard:
//! `MSTD-<TIPO>-<NNNN>`.
//!
//! O tipo é a sigla em inglês do tipo do evento (`RULE`, `CRIT`, `WAVE`…) e o
//! número tem quatro dígitos, contado dentro de cada spec e de cada tipo. O
//! código só tem letras, números e hífen, então ele mesmo serve de endereço do
//! item na página.
//!
//! Só esse formato é código. Um texto qualquer com letra e número
//! ("R2 da Cloudflare", "S3", "A4") fica como está: não vira link e não é apontado
//! pela conferência do fim da resposta.

/// O começo de todo código.
pub const PREFIX: &str = "MSTD";

/// Quantos dígitos o número tem, no mínimo.
pub const DIGITS: usize = 4;

/// O código do item de número `number` do tipo de sigla `kind`:
/// `format("RULE", n)` é `MSTD-RULE-NNNN`, com `n` escrito com zeros à frente
/// até ter [`DIGITS`] dígitos.
#[must_use]
pub fn format(kind: &str, number: u64) -> String {
    format!("{PREFIX}-{kind}-{number:0DIGITS$}")
}

/// `true` quando `token` é um código inteiro, sem nada antes nem depois.
#[must_use]
pub fn is_id(token: &str) -> bool {
    find(token) == [(0, token.len())]
}

/// A sigla e o número de um código inteiro: `parse("MSTD-RULE-NNNN")` é
/// `Some(("RULE", n))`, com `n` o número sem os zeros à frente. `None` para o
/// que não é código.
#[must_use]
pub fn parse(token: &str) -> Option<(&str, u64)> {
    if !is_id(token) {
        return None;
    }
    let (kind, digits) = token.strip_prefix(PREFIX)?.strip_prefix('-')?.rsplit_once('-')?;
    Some((kind, digits.parse().ok()?))
}

/// Os trechos `(início, fim)`, em bytes, de cada código de `text`, na ordem.
///
/// Um código precisa começar e terminar numa fronteira de palavra: `xMSTD-…`
/// e o número seguido de letra (`MSTD-RULE-NNNNa`) não contam. O hífen conta
/// como fronteira no fim: o código seguido de hífen ainda conta.
#[must_use]
pub fn find(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(pos) = text[from..].find(PREFIX) {
        let start = from + pos;
        from = start + PREFIX.len();
        let glued_before = start > 0 && is_word_byte(bytes[start - 1]);
        if glued_before {
            continue;
        }
        if let Some(end) = code_end(bytes, start + PREFIX.len()) {
            spans.push((start, end));
            from = end;
        }
    }
    spans
}

/// Onde termina o código que começa com `MSTD` e segue em `at`: um hífen, a
/// sigla em maiúsculas, outro hífen e pelo menos [`DIGITS`] dígitos, com uma
/// fronteira depois. `None` quando o resto não tem esse formato.
fn code_end(bytes: &[u8], at: usize) -> Option<usize> {
    let mut i = at;
    if bytes.get(i) != Some(&b'-') {
        return None;
    }
    i += 1;
    let kind_start = i;
    while bytes.get(i).is_some_and(u8::is_ascii_uppercase) {
        i += 1;
    }
    if i == kind_start || bytes.get(i) != Some(&b'-') {
        return None;
    }
    i += 1;
    let digits_start = i;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let glued_after = bytes.get(i).is_some_and(|b| is_word_byte(*b));
    (i - digits_start >= DIGITS && !glued_after).then_some(i)
}

/// Letra, dígito ou `_` (ASCII ou não): o que gruda numa palavra.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_has_the_prefix_the_kind_and_four_digits() {
        assert_eq!(format("RULE", 5), "MSTD-RULE-0005");
        assert_eq!(format("CRIT", 12), "MSTD-CRIT-0012");
        assert_eq!(format("WAVE", 12345), "MSTD-WAVE-12345");
        assert!(is_id("MSTD-RULE-0005"));
        assert!(!is_id("MSTD-RULE-0005 "));
    }

    #[test]
    fn a_code_reads_back_as_its_kind_and_number() {
        assert_eq!(parse("MSTD-RULE-0005"), Some(("RULE", 5)));
        assert_eq!(parse("MSTD-WAVE-12345"), Some(("WAVE", 12345)));
        assert_eq!(parse(&format("CRIT", 7)), Some(("CRIT", 7)));
        for token in ["MSTD-RULE-005", "R8", "MSTD-RULE-0005 ", "", "12"] {
            assert_eq!(parse(token), None, "{token}");
        }
    }

    #[test]
    fn codes_are_found_inside_prose_at_word_boundaries() {
        let text = "Veja MSTD-RULE-0005, a (MSTD-CRIT-0012) e MSTD-WAVE-0002.";
        let found: Vec<&str> = find(text).into_iter().map(|(s, e)| &text[s..e]).collect();
        assert_eq!(found, ["MSTD-RULE-0005", "MSTD-CRIT-0012", "MSTD-WAVE-0002"]);
    }

    /// Letra com número fora do formato não é código: nome de produto,
    /// tamanho de papel, rótulo antigo.
    #[test]
    fn letters_and_numbers_outside_the_format_are_not_codes() {
        for text in [
            "Guardei o arquivo no R2 da Cloudflare.",
            "O S3 e o papel A4.",
            "A regra R8 e o critério C-15.",
            "MSTD-rule-0005",
            "MSTD-RULE-005",
            "MSTD--0005",
            "xMSTD-RULE-0005",
            "MSTD-RULE-0005a",
            "MSTD-RULE0005",
            "MSTD",
        ] {
            assert!(find(text).is_empty(), "{text}");
        }
    }
}
