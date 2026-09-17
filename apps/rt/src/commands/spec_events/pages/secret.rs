//! Texto com cara de segredo: chave, token ou senha.
//!
//! Antes de uma página poder ser publicada, cada trecho dela passa por aqui, e
//! o que casa fica fora da página até o item ser expurgado. As famílias são as
//! de chave e token com forma conhecida, o cabeçalho de chave privada e a
//! senha escrita como atribuição (`senha: …`, `password=…`, `a senha é …`),
//! cujo valor tem letra e número e não é um marcador como `<senha>` ou
//! `${TOKEN}`.

use std::sync::OnceLock;

use regex::Regex;

/// As formas de chave e token que não se confundem com texto comum.
const SHAPES: &[&str] = &[
    r"\bAKIA[0-9A-Z]{16}\b",
    r"\bgh[pousr]_[A-Za-z0-9]{36,}",
    r"\bgithub_pat_[A-Za-z0-9_]{22,}",
    r"\bglpat-[A-Za-z0-9_-]{20,}",
    r"\b[sr]k_(?:live|test)_[A-Za-z0-9]{24,}",
    r"\bxox[abprs]-[A-Za-z0-9-]{10,}",
    r"\bsk-ant-[A-Za-z0-9_-]{20,}",
    r"\bsk-(?:proj-)?[A-Za-z0-9]{32,}",
    r"\bAIza[0-9A-Za-z_-]{35}",
    r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
    r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
];

/// A senha, o token ou a chave escritos como atribuição; o valor vem no
/// segundo grupo.
const ASSIGNMENT: &str = r#"(?i)\b(password|passwd|pwd|senha|secret|segredo|token|api[_-]?key|access[_-]?key)\s*(?:[:=]|\sé\s|\sis\s)\s*["']?([^\s"'`<>]{8,})"#;

fn shapes() -> Option<&'static Regex> {
    static SHAPES_RE: OnceLock<Option<Regex>> = OnceLock::new();
    SHAPES_RE.get_or_init(|| Regex::new(&SHAPES.join("|")).ok()).as_ref()
}

fn assignment() -> Option<&'static Regex> {
    static ASSIGNMENT_RE: OnceLock<Option<Regex>> = OnceLock::new();
    ASSIGNMENT_RE.get_or_init(|| Regex::new(ASSIGNMENT).ok()).as_ref()
}

/// O texto tem algo com cara de segredo.
pub(super) fn looks_like_secret(text: &str) -> bool {
    shapes().is_some_and(|re| re.is_match(text))
        || assignment().is_some_and(|re| {
            re.captures_iter(text).any(|caps| caps.get(2).is_some_and(|value| real_value(value.as_str())))
        })
}

/// Um valor de atribuição que parece de verdade: tem letra e número e não é um
/// marcador de lugar.
fn real_value(value: &str) -> bool {
    let placeholder = value.starts_with(['$', '{', '%', '*', '.'])
        || value.chars().all(|c| c == value.chars().next().unwrap_or('x'))
        || value.to_ascii_lowercase().contains("xxxx");
    let letter = value.chars().any(char::is_alphabetic);
    let digit = value.chars().any(|c| c.is_ascii_digit());
    letter && digit && !placeholder
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cada família conhecida casa; o mesmo assunto escrito em prosa, um nome
    /// de campo, um marcador e um exemplo curto não casam.
    #[test]
    fn keys_tokens_and_passwords_are_found_and_prose_is_not() {
        let found = [
            "a chave AKIAIOSFODNN7EXAMPLE vazou",
            &format!("token ghp_{}", "a1".repeat(18)),
            &format!("glpat-{}", "x1y2".repeat(5)),
            &format!("sk_live_{}", "4eC39HqLyjWDarjtT1zdp7dc"),
            "xoxb-123456789012-abcdef",
            &format!("sk-ant-api03-{}", "Zx9".repeat(8)),
            "-----BEGIN RSA PRIVATE KEY-----",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
            "a senha: hunter2-segredo",
            "a senha é hunter2-segredo",
            "PASSWORD=S3nh4F0rte!",
            "api_key = \"abc123def456\"",
        ];
        for text in found {
            assert!(looks_like_secret(text), "not found: {text}");
        }
        let prose = [
            "o binário procura texto com cara de segredo (chave, token, senha)",
            "a senha: <senha>",
            "token: ${GITHUB_TOKEN}",
            "password=********",
            "a senha é o campo do formulário",
            "the password is required",
            "o campo `search` e o campo token: vazio",
            "secret: abcdefgh",
            "MSTD-RULE-0008 e sk-curto",
        ];
        for text in prose {
            assert!(!looks_like_secret(text), "false positive: {text}");
        }
    }
}
