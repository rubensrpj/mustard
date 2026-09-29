//! `triage` — o grau da resposta da busca do mapa, de 0 a 5, e o corte do que
//! a busca funda traz.
//!
//! O grau sai de três sinais da busca por palavra com campos: a nota do
//! primeiro achado, a parte das palavras da pergunta que ele traz em campo
//! forte (nome, assinatura, erro, rótulo, rota, log) e a distância dele para
//! o segundo. Os pesos e os pontos de corte saem de uma régua de 360 buscas
//! sobre três projetos, com o arquivo certo conhecido de cada uma: um ajuste
//! logístico dá a chance de o primeiro achado ser o arquivo certo, e o grau é
//! a faixa dessa chance. Nenhum número aqui é chute.
//!
//! Na régua, o primeiro achado é o certo em 88% das buscas de grau 5, 80% do
//! grau 4, 51% do grau 3, 19% do grau 2 e 12% do grau 1; o arquivo certo está
//! entre os cinco em 91%, 94%, 89%, 69% e 53%. Cada projeto deixado de fora
//! do ajuste e medido com os pesos dos outros dois dá a mesma ordem das
//! faixas.

/// Os sinais da busca por palavra de que o grau sai.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Signals {
    /// Quantas palavras a pergunta tem, sem as de ligação.
    pub words: usize,
    /// Quantas delas o primeiro achado traz em campo forte.
    pub strong: usize,
    /// A nota do primeiro achado; `None` sem achado.
    pub first: Option<f64>,
    /// A nota do segundo achado.
    pub second: Option<f64>,
}

/// Do grau 3 para baixo a busca funda roda.
pub const DEEP_UNTIL: u8 = 3;

/// O termo fixo do ajuste.
const BIAS: f64 = -4.9221;

/// O peso da parte das palavras da pergunta que o primeiro achado traz em
/// campo forte.
const WEIGHT_STRONG: f64 = 5.5837;

/// O peso da nota do primeiro achado por palavra da pergunta.
const WEIGHT_FIRST: f64 = 0.3436;

/// O peso da distância do primeiro achado para o segundo, em parte da nota
/// do primeiro.
const WEIGHT_GAP: f64 = 6.5376;

/// Os pontos de corte da chance entre um grau e o seguinte: de 1 a 5, cada
/// ponto alcançado sobe um grau.
const EDGES: [f64; 4] = [0.2, 0.4, 0.6, 0.8];

/// A chance de o primeiro achado ser o certo, de 0 a 1. Sem achado, zero.
#[must_use]
pub fn chance(signals: &Signals) -> f64 {
    let Some(first) = signals.first else { return 0.0 };
    let words = signals.words.max(1) as f64;
    let gap = match signals.second {
        Some(second) if first > 0.0 => (first - second) / first,
        _ => 1.0,
    };
    let share = signals.strong as f64 / words;
    let logit = BIAS + WEIGHT_STRONG * share + WEIGHT_FIRST * (first / words) + WEIGHT_GAP * gap;
    1.0 / (1.0 + (-logit).exp())
}

/// O grau de uma chance: 1 para a menor faixa, mais um por ponto de corte
/// alcançado.
#[must_use]
pub fn band(chance: f64) -> u8 {
    1 + EDGES.iter().filter(|edge| chance >= **edge).count() as u8
}

/// O grau da resposta: 0 sem achado, e de 1 a 5 pela faixa da chance.
#[must_use]
pub fn grade(signals: &Signals) -> u8 {
    if signals.first.is_none() {
        return 0;
    }
    band(chance(signals))
}

/// A parte da nota do primeiro abaixo da qual um achado da busca funda é
/// muito mais fraco e fica fora: a maior que não perde nenhum arquivo certo
/// que a busca funda resgata na régua, e que corta as sobras — os arquivos
/// sem relação — de 4,8 para 3,8 por resposta de grau baixo com busca funda.
const KEEP_RATIO: f64 = 0.7;

/// Se um achado da busca funda, com a nota `score`, fica na resposta: o
/// muito mais fraco que o primeiro, de nota `first`, fica fora.
#[must_use]
pub fn keeps(score: f64, first: f64) -> bool {
    score >= KEEP_RATIO * first
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signals(words: usize, strong: usize, first: f64, second: Option<f64>) -> Signals {
        Signals { words, strong, first: Some(first), second }
    }

    #[test]
    fn no_first_finding_is_grade_zero() {
        let none = Signals { words: 3, strong: 0, first: None, second: None };
        assert_eq!(grade(&none), 0);
        assert!(chance(&none).abs() < f64::EPSILON);
    }

    #[test]
    fn each_cut_point_lifts_the_grade_by_one_when_reached() {
        assert_eq!(band(0.0), 1);
        assert_eq!(band(0.199_999), 1);
        assert_eq!(band(0.2), 2);
        assert_eq!(band(0.399_999), 2);
        assert_eq!(band(0.4), 3);
        assert_eq!(band(0.599_999), 3);
        assert_eq!(band(0.6), 4);
        assert_eq!(band(0.799_999), 4);
        assert_eq!(band(0.8), 5);
        assert_eq!(band(1.0), 5);
    }

    #[test]
    fn a_question_all_found_in_strong_fields_far_ahead_of_the_second_is_grade_five() {
        assert_eq!(grade(&signals(2, 2, 40.0, Some(10.0))), 5);
    }

    #[test]
    fn a_question_none_found_in_strong_fields_and_tied_is_grade_one() {
        assert_eq!(grade(&signals(3, 0, 4.0, Some(4.0))), 1);
    }

    #[test]
    fn the_grade_falls_when_the_first_finding_loses_its_lead() {
        let ahead = grade(&signals(2, 1, 12.0, Some(3.0)));
        let tied = grade(&signals(2, 1, 12.0, Some(12.0)));
        assert!(ahead > tied, "{ahead} should be above {tied}");
    }

    #[test]
    fn the_grade_falls_with_each_question_word_the_first_finding_lacks() {
        let grades: Vec<u8> = (0..=3).rev().map(|strong| grade(&signals(3, strong, 20.0, Some(8.0)))).collect();
        assert!(grades.windows(2).all(|pair| pair[0] >= pair[1]), "{grades:?}");
        assert!(grades[0] > grades[3], "{grades:?}");
    }

    #[test]
    fn a_lone_finding_counts_as_fully_ahead() {
        let alone = chance(&signals(1, 1, 9.0, None));
        assert!((alone - chance(&signals(1, 1, 9.0, Some(0.0)))).abs() < 1e-12);
    }

    #[test]
    fn a_finding_at_seven_tenths_of_the_first_stays_and_just_below_it_goes() {
        assert!(keeps(7.0, 10.0));
        assert!(keeps(10.0, 10.0));
        assert!(!keeps(6.99, 10.0));
        assert!(!keeps(0.0, 10.0));
    }
}
