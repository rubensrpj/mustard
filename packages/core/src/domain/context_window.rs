//! `context_window` — a janela de contexto de cada modelo do Claude, em
//! tokens: quanto a conversa inteira pode ocupar, a resposta inclusive.
//!
//! A tabela segue a documentação da API, conferida em 15/10/2026:
//! <https://platform.claude.com/docs/en/build-with-claude/context-windows>
//! (os modelos de 1 milhão; "os outros têm 200 mil") e
//! <https://platform.claude.com/docs/en/about-claude/model-deprecations>
//! (os nomes da API dos modelos ainda no ar). Os aposentados ficam de fora:
//! modelo fora da tabela não tem janela conhecida, e quem pergunta decide o
//! que vale sem ela.

/// O nome do modelo na API, sem a data da versão, e a janela dele.
const WINDOWS: &[(&str, u64)] = &[
    ("claude-fable-5-1", 1_000_000),
    ("claude-mythos-5-1", 1_000_000),
    ("claude-fable-5", 1_000_000),
    ("claude-mythos-5", 1_000_000),
    ("claude-mythos-preview", 1_000_000),
    ("claude-opus-5-5", 1_000_000),
    ("claude-opus-5", 1_000_000),
    ("claude-opus-4-8", 1_000_000),
    ("claude-opus-4-7", 1_000_000),
    ("claude-opus-4-6", 1_000_000),
    ("claude-sonnet-5-5", 1_000_000),
    ("claude-sonnet-5", 1_000_000),
    ("claude-sonnet-4-6", 1_000_000),
    ("claude-haiku-5-5", 1_000_000),
    // Na API: claude-opus-4-5-20251101, claude-sonnet-4-5-20250929 e
    // claude-haiku-4-5-20251001.
    ("claude-opus-4-5", 200_000),
    ("claude-sonnet-4-5", 200_000),
    ("claude-haiku-4-5", 200_000),
];

/// A janela do modelo `model`, em tokens, como a conversa o grava: o nome da
/// API, com ou sem a data da versão (`-20250929`), e as formas das nuvens —
/// o prefixo do Bedrock (`us.anthropic.`) com o sufixo dele (`-v1:0`) e a
/// data do Google Cloud depois de `@`. `None` para o que a tabela não
/// conhece.
#[must_use]
pub fn window_of(model: &str) -> Option<u64> {
    let model = model.trim();
    let name = &model[model.find("claude-")?..];
    let name = name.split([':', '@']).next().unwrap_or(name);
    let name = strip_suffix(name, "-v", 1).unwrap_or(name);
    let name = strip_suffix(name, "-", 8).unwrap_or(name);
    WINDOWS.iter().find(|(id, _)| *id == name).map(|(_, window)| *window)
}

/// `name` sem o fim `<mark><dígitos>`, com ao menos `digits` dígitos; `None`
/// quando ele não termina assim.
fn strip_suffix<'a>(name: &'a str, mark: &str, digits: usize) -> Option<&'a str> {
    let (rest, number) = name.rsplit_once(mark)?;
    (number.len() >= digits && number.chars().all(|c| c.is_ascii_digit())).then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::{WINDOWS, window_of};

    /// A menor janela da API é a de 200 mil: um número menor na tabela é
    /// erro de digitação, e mandaria o sinal de conclusão antes do horizonte
    /// para o qual a janela de 200 mil foi pensada.
    #[test]
    fn every_window_in_the_table_holds_at_least_two_hundred_thousand() {
        for (model, window) in WINDOWS {
            assert!(*window >= 200_000, "{model}: {window}");
        }
    }

    /// Cada forma com que a conversa grava um modelo da tabela acha a janela
    /// dele: o nome da API, o datado, o do Bedrock e o do Google Cloud.
    #[test]
    fn the_api_name_and_its_cloud_forms_find_the_window() {
        for (model, window) in [
            ("claude-opus-5-5", 1_000_000),
            ("claude-sonnet-4-6", 1_000_000),
            ("claude-sonnet-4-5-20250929", 200_000),
            ("claude-sonnet-4-5", 200_000),
            ("us.anthropic.claude-haiku-4-5-20251001-v1:0", 200_000),
            ("anthropic.claude-opus-5", 1_000_000),
            ("claude-haiku-4-5@20251001", 200_000),
        ] {
            assert_eq!(window_of(model), Some(window), "{model}");
        }
    }

    /// Modelo aposentado, de outra plataforma, com sufixo que a API não
    /// grava ou sem nome não tem janela conhecida: o nome mais curto não pega
    /// a janela do mais longo.
    #[test]
    fn a_model_outside_the_table_has_no_window() {
        for model in ["claude-opus-4-20250514", "claude-opus-4", "claude-3-7-sonnet-20250219", "gpt-5-codex", "<synthetic>", "", "claude-opus-5-5[1m]"] {
            assert_eq!(window_of(model), None, "{model}");
        }
    }
}
