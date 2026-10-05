//! A medição do uso real: a tabela do gasto antes e depois da marca de uma
//! versão e a frase do veredito, como o comando `measure` as responde.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["measure"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // O veredito, numa frase, com as lacunas que o comando troca.
        ("measure.no_mark", Locale::PtBr) => {
            "A medição começa na próxima sessão, quando esta versão do Mustard deixa a marca dela neste projeto."
        }
        ("measure.no_mark", Locale::EnUs) => {
            "The measurement starts at the next session, when this Mustard version leaves its mark on this project."
        }
        ("measure.not_used_yet", Locale::PtBr) => {
            "Esta versão ainda não foi usada neste projeto. A coluna de antes já está pronta."
        }
        ("measure.not_used_yet", Locale::EnUs) => {
            "This version has not been used in this project yet. The before column is already ready."
        }
        ("measure.too_early", Locale::PtBr) => {
            "Ainda não dá para dizer. São {before_days} dias contados antes e {after_days} depois, e o mínimo \
             é {min} de cada lado: faltam {missing_before} antes e {missing_after} depois."
        }
        ("measure.too_early", Locale::EnUs) => {
            "Too early to tell. There are {before_days} counted days before and {after_days} after, and the \
             minimum is {min} on each side: {missing_before} are missing before and {missing_after} after."
        }
        ("measure.so_far", Locale::PtBr) => "Até aqui, cada ação custou {change} depois da instalação.",
        ("measure.so_far", Locale::EnUs) => "So far, each action cost {change} after the installation.",
        ("measure.ready", Locale::PtBr) => {
            "A comparação vale, com {before_days} dias contados antes e {after_days} depois: cada ação custou \
             {change} depois da instalação."
        }
        ("measure.ready", Locale::EnUs) => {
            "The comparison holds, with {before_days} counted days before and {after_days} after: each action \
             cost {change} after the installation."
        }
        // A diferença do custo de cada ação, do antes para o depois.
        ("measure.more", Locale::PtBr) => "{percent}% mais",
        ("measure.more", Locale::EnUs) => "{percent}% more",
        ("measure.less", Locale::PtBr) => "{percent}% menos",
        ("measure.less", Locale::EnUs) => "{percent}% less",
        ("measure.same", Locale::PtBr) => "o mesmo",
        ("measure.same", Locale::EnUs) => "the same",

        // A tabela do gasto: as colunas, as linhas, as unidades e a palavra
        // que junta o último dia da lista.
        ("measure.before", Locale::PtBr) => "antes",
        ("measure.before", Locale::EnUs) => "before",
        ("measure.after", Locale::PtBr) => "depois",
        ("measure.after", Locale::EnUs) => "after",
        ("measure.days", Locale::PtBr) => "dias contados",
        ("measure.days", Locale::EnUs) => "counted days",
        ("measure.actions", Locale::PtBr) => "ações do Claude",
        ("measure.actions", Locale::EnUs) => "Claude actions",
        ("measure.tokens", _) => "tokens",
        ("measure.per_action", Locale::PtBr) => "tokens por ação",
        ("measure.per_action", Locale::EnUs) => "tokens per action",
        ("measure.unused", Locale::PtBr) => "sem uso",
        ("measure.unused", Locale::EnUs) => "no use",
        ("measure.millions", Locale::PtBr) => "{n} milhões",
        ("measure.millions", Locale::EnUs) => "{n} million",
        ("measure.thousands", Locale::PtBr) => "{n} mil",
        ("measure.thousands", Locale::EnUs) => "{n} thousand",
        ("measure.and", Locale::PtBr) => "e",
        ("measure.and", Locale::EnUs) => "and",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas. Quem muda um texto de propósito grava aqui os dois números
    /// novos que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged(
            include_str!("measure.rs"),
            super::PREFIXES,
            18,
            0xe4f6_c8ce_67c7_8ee7,
        );
    }
}
