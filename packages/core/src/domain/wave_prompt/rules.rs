//! As regras do projeto no pedido do revisor: o texto do `CLAUDE.md` da raiz
//! do projeto, numa seção própria no fim. O agente de revisão é instalado sem
//! os arquivos de instrução da conversa principal — com eles viria a memória
//! de quem conduz a obra —, e é por esta seção que as regras do projeto
//! chegam a ele. O pedido da onda não a leva.

use crate::platform::i18n::{translate, Locale};

/// A seção das regras do projeto: o título, a origem e o texto como o
/// projeto o escreveu, sem os brancos das pontas.
#[must_use]
pub fn project_rules_section(rules: &str, lang: Locale) -> String {
    format!(
        "## {}\n\n{}\n\n{}",
        translate("prompt.part.project_rules", lang),
        translate("prompt.project_rules.source", lang),
        rules.trim()
    )
}

/// Se `text` já traz a seção das regras do projeto, pela linha do título dela.
#[must_use]
pub fn carries_project_rules(text: &str, lang: Locale) -> bool {
    let heading = format!("## {}", translate("prompt.part.project_rules", lang));
    text.lines().any(|line| line.trim_end() == heading)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::wave_prompt::{write, write_final_review, Material};

    /// Com as regras do projeto no material, o pedido da revisão final é o de
    /// sempre seguido da seção delas no fim; sem elas, nenhuma linha muda. O
    /// pedido da onda, com o mesmo material, não as leva.
    #[test]
    fn the_final_review_request_ends_with_the_project_rules_and_the_wave_request_never_carries_them() {
        let rules = "# Regras\n\n- O instalador nunca grava na configuração do git.";
        for lang in [Locale::PtBr, Locale::EnUs] {
            let bare = Material { spec: "x".into(), ..Material::default() };
            let with_rules = Material { spec: "x".into(), project_rules: Some(format!("\n{rules}\n\n")), ..Material::default() };
            let today = write_final_review(&bare, lang);
            assert!(!carries_project_rules(&today, lang), "{today}");

            let review = write_final_review(&with_rules, lang);
            let heading = translate("prompt.part.project_rules", lang);
            let source = translate("prompt.project_rules.source", lang);
            assert_eq!(review, format!("{today}\n## {heading}\n\n{source}\n\n{rules}\n"));
            assert!(carries_project_rules(&review, lang), "{review}");

            let wave = write(&with_rules, lang);
            assert_eq!(wave, write(&bare, lang), "the wave request is the same with or without the rules");
            assert!(!wave.contains("configuração do git") && !carries_project_rules(&wave, lang), "{wave}");
        }
    }
}
