//! As regras do projeto no pedido do revisor: o texto de cada arquivo de
//! regras da raiz e das pastas onde a obra mexeu, numa seção própria no fim.
//! O agente de revisão é instalado sem os arquivos de instrução da conversa
//! principal — com eles viria a memória de quem conduz a obra —, e é por esta
//! seção que as regras do projeto chegam a ele. O pedido da onda não a leva.

use std::fmt::Write as _;

use crate::domain::spec_events::{Block, BlockQuery, SpecLog};
use crate::platform::i18n::{Locale, translate};

/// O arquivo de regras da raiz do projeto: o do projeto que tem um só.
pub const ROOT_RULES_FILE: &str = "CLAUDE.md";

/// Um arquivo de regras do projeto, como a seção o mostra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesFile {
    /// O caminho relativo à raiz do projeto, com barras normais.
    pub path: String,
    /// O texto, com o que ele manda incluir já no lugar.
    pub text: String,
}

/// A seção das regras do projeto, com o texto de cada arquivo sem os brancos
/// das pontas. Com um só arquivo, o da raiz, ela traz o título, a origem e o
/// texto, como o projeto o escreveu. Com mais de um, a origem diz que são os
/// arquivos das pastas da obra, e cada texto vem sob uma linha com o caminho
/// dele. `None` sem arquivo nenhum: o pedido sai sem a seção.
#[must_use]
pub fn project_rules_section(rules: &[RulesFile], lang: Locale) -> Option<String> {
    let heading = translate("prompt.part.project_rules", lang);
    match rules {
        [] => None,
        [only] if only.path == ROOT_RULES_FILE => Some(format!("## {heading}\n\n{}\n\n{}", translate("prompt.project_rules.source", lang), only.text.trim())),
        many => {
            let mut out = format!("## {heading}\n\n{}", translate("prompt.project_rules.sources", lang));
            for file in many {
                let _ = write!(out, "\n\n### `{}`\n\n{}", file.path, file.text.trim());
            }
            Some(out)
        }
    }
}

/// Os arquivos que as tarefas e as entregas da spec declaram, na ordem do
/// arquivo, sem repetir: onde a obra mexe, para a leitura das regras.
#[must_use]
pub fn touched_by(log: &SpecLog) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let events = log.block(BlockQuery::Block(Block::Waves));
    for event in events.into_iter().filter(|e| matches!(e.event_type.as_str(), "task" | "delivered")) {
        for path in super::declared_paths(event) {
            if !out.iter().any(|seen| seen == path) {
                out.push(path.to_string());
            }
        }
    }
    out
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
    use crate::domain::wave_prompt::{Material, write, write_final_review};

    /// Com só o arquivo de regras da raiz no material, o pedido da revisão
    /// final é o de sempre seguido da seção dele no fim; sem arquivo, nenhuma
    /// linha muda. Com mais de um, ou com um que não é o da raiz, a origem diz
    /// que são os das pastas da obra, e cada texto vem sob o caminho dele. O
    /// pedido da onda, com o mesmo material, não as leva.
    #[test]
    fn both_final_review_and_wave_requests_carry_the_complete_mandatory_project_rules() {
        let rules = "# Regras\n\n- O instalador nunca grava na configuração do git.";
        let file = |path: &str, text: &str| RulesFile { path: path.to_string(), text: text.to_string() };
        for lang in [Locale::PtBr, Locale::EnUs] {
            let bare = Material { spec: "x".into(), ..Material::default() };
            let with_rules = Material { spec: "x".into(), project_rules: vec![file("CLAUDE.md", &format!("\n{rules}\n\n"))], ..Material::default() };
            let today = write_final_review(&bare, lang);
            assert!(!carries_project_rules(&today, lang), "{today}");
            assert_eq!(project_rules_section(&[], lang), None, "no rules file, no section");

            let review = write_final_review(&with_rules, lang);
            let heading = translate("prompt.part.project_rules", lang);
            let source = translate("prompt.project_rules.source", lang);
            assert_eq!(review, format!("{today}\n## {heading}\n\n{source}\n\n{rules}\n"));
            assert!(carries_project_rules(&review, lang), "{review}");

            let sources = translate("prompt.project_rules.sources", lang);
            let files = [file("CLAUDE.md", "- Raiz.\n"), file("apps/cli/CLAUDE.md", "\n- Instalador.")];
            let both = format!("## {heading}\n\n{sources}\n\n### `CLAUDE.md`\n\n- Raiz.\n\n### `apps/cli/CLAUDE.md`\n\n- Instalador.");
            assert_eq!(project_rules_section(&files, lang), Some(both));
            let nested = format!("## {heading}\n\n{sources}\n\n### `apps/cli/CLAUDE.md`\n\n- Instalador.");
            assert_eq!(project_rules_section(&files[1..], lang), Some(nested), "one file that is not the root's");

            let wave = write(&with_rules, lang);
            assert_ne!(wave, write(&bare, lang));
            assert!(wave.contains(rules) && carries_project_rules(&wave, lang), "mandatory rules reach the executor: {wave}");
        }
    }
}
