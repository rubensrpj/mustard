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
//! Na régua, o primeiro achado é o certo em 88% das buscas de grau 5, 64% do
//! grau 4, 54% do grau 3, 28% do grau 2 e 18% do grau 1; o arquivo certo está
//! entre os cinco em 94%, 88%, 83%, 65% e 36%. Os pesos por campo da busca dos
//! arquivos mudaram a escala da nota e levaram os pesos daqui a serem
//! refeitos sobre ela. A ordem das faixas vale no ajuste inteiro; deixado um
//! projeto de fora e medido com os pesos dos outros dois, a ordem se mantém
//! só em parte, e o corte da cobertura do cravado, escolhido no ajuste
//! inteiro, não é seguro para um projeto que o ajuste nunca viu.

use crate::platform::i18n::{translate, Locale};

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

/// O que a conferência dos primeiros candidatos diz da frente da lista: que
/// parte das palavras raras da pergunta o primeiro traz e que parte o segundo
/// traz, cada parte pela raridade das palavras (de 0 a 1).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Lead {
    /// A parte das palavras raras que o primeiro candidato traz.
    pub first: f64,
    /// A parte que o segundo candidato traz.
    pub second: f64,
}

/// O candidato cobre as palavras raras da pergunta quando traz esta parte
/// delas, pela raridade. Sai da régua das 360 buscas (nomes e frase, três
/// projetos): com a nota 5 e o primeiro a 0,7 ou mais e o segundo abaixo
/// disso, ficam 19 buscas cravadas e o primeiro achado é o certo nas 19; a
/// 0,8 ficam 8, a 0,6 ficam 16, a 0,5 ficam 13 e 12 acertam.
pub const COVERS_FROM: f64 = 0.7;

impl Lead {
    /// `true` quando o primeiro cobre as palavras raras e o segundo não: só o
    /// primeiro serve à pergunta. Sozinha, a frente não crava: dos dez casos
    /// que ela pega na régua só quatro têm o primeiro achado certo, e a nota
    /// 5 é o que filtra os outros.
    #[must_use]
    pub fn leads(self) -> bool {
        self.first >= COVERS_FROM && self.second < COVERS_FROM
    }
}

/// Do grau 3 para baixo a busca funda roda.
pub const DEEP_UNTIL: u8 = 3;

/// O maior grau de uma resposta cujo primeiro arquivo não é o primeiro do
/// banco: a chance do grau fala do primeiro do banco, então a resposta que
/// põe outro na frente nunca chega ao grau 5, o do cravado.
pub const UNSURE_GRADE: u8 = 4;

/// O termo fixo do ajuste.
const BIAS: f64 = -3.3276;

/// O peso da parte das palavras da pergunta que o primeiro achado traz em
/// campo forte.
const WEIGHT_STRONG: f64 = 2.3850;

/// O peso da nota do primeiro achado por palavra da pergunta.
const WEIGHT_FIRST: f64 = 0.6057;

/// O peso da distância do primeiro achado para o segundo, em parte da nota
/// do primeiro.
const WEIGHT_GAP: f64 = 3.7602;

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

/// O quanto o mapa achou de uma busca por palavra, em três marcas: a nota de
/// 0 a 5 dita em palavras que quem busca entende na hora.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// A nota é a mais alta e a conferência dos primeiros candidatos diz que
    /// só o primeiro cobre as palavras raras da pergunta: a resposta do mapa
    /// vale no lugar da busca comum, mesmo com palavras da pergunta fora dos
    /// campos fortes dele.
    Pinned,
    /// O mapa achou parte: a busca comum roda, e o que o mapa achou vai junto
    /// dela, com as palavras que faltam.
    Partial,
    /// O mapa não achou nada: a busca comum é a saída.
    NotFound,
}

impl Mark {
    /// A chave da marca na resposta do mapa.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Pinned => "pinned",
            Self::Partial => "partial",
            Self::NotFound => "not_found",
        }
    }
}

/// A marca de uma resposta de nota `grade`: nota 0 é não achou; cravado
/// exige a nota mais alta e a frente da conferência ([`Lead::leads`]), o
/// primeiro cobrindo as palavras raras da pergunta e o segundo não; o resto
/// é parcial.
///
/// A frente no lugar do corte da chance (0,93) porque o corte só cravava a
/// resposta em que a nota era alta, e a nota mede o primeiro achado sozinho;
/// a frente mede o primeiro contra o segundo. Na régua das 360 buscas o corte
/// cravava 6 buscas (todas do Mustard, 6 certas) e nenhuma na frase; a frente
/// com a nota 5 crava 19 (18 nos nomes, 1 na frase) e as 19 têm o primeiro
/// achado certo, nos três projetos.
#[must_use]
pub fn mark(grade: u8, lead: Lead) -> Mark {
    if grade == 0 {
        Mark::NotFound
    } else if grade >= 5 && lead.leads() {
        Mark::Pinned
    } else {
        Mark::Partial
    }
}

/// A linha de quando o mapa não achou: as palavras da pergunta, já
/// quebradas, e a próxima busca, exata, no texto dos arquivos. A pergunta
/// que só tinha palavras de ligação entra como veio.
#[must_use]
pub fn not_found(query: &str, words: &[String], lang: Locale) -> String {
    let words: Vec<&str> = if words.is_empty() { vec![query.trim()] } else { words.iter().map(String::as_str).collect() };
    let shown = words.iter().map(|word| format!("\"{word}\"")).collect::<Vec<_>>().join(", ");
    let next = format!("grep -rniE \"{}\" .", words.join("|"));
    translate("map.search.not_found", lang).replace("{words}", &shown).replace("{next}", &next)
}

/// A parte da nota do primeiro abaixo da qual um achado da busca funda é
/// muito mais fraco e fica fora: corta as sobras — os arquivos sem relação —
/// de 4,7 para 3,3 por resposta de grau baixo com busca funda, na régua das
/// 360 buscas. Com a busca dos arquivos lendo os comentários, nenhuma busca
/// da régua é resgatada pela busca funda, então nenhum corte perde arquivo
/// certo; o 0,7 fica pelo corte que já valia.
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
        let ahead = grade(&signals(2, 1, 4.0, Some(1.0)));
        let tied = grade(&signals(2, 1, 4.0, Some(4.0)));
        assert!(ahead > tied, "{ahead} should be above {tied}");
    }

    #[test]
    fn the_grade_falls_with_each_question_word_the_first_finding_lacks() {
        let grades: Vec<u8> = (0..=3).rev().map(|strong| grade(&signals(3, strong, 6.0, Some(2.0)))).collect();
        assert!(grades.windows(2).all(|pair| pair[0] >= pair[1]), "{grades:?}");
        assert!(grades[0] > grades[3], "{grades:?}");
    }

    #[test]
    fn a_lone_finding_counts_as_fully_ahead() {
        let alone = chance(&signals(1, 1, 9.0, None));
        assert!((alone - chance(&signals(1, 1, 9.0, Some(0.0)))).abs() < 1e-12);
    }

    fn ahead() -> Lead {
        Lead { first: 0.9, second: 0.2 }
    }

    #[test]
    fn no_finding_is_not_found_and_any_grade_below_five_is_partial_even_ahead() {
        assert_eq!(mark(0, Lead::default()), Mark::NotFound);
        assert_eq!(mark(0, ahead()), Mark::NotFound);
        for grade in 1..=4 {
            assert_eq!(mark(grade, ahead()), Mark::Partial, "grade {grade}");
        }
    }

    #[test]
    fn grade_five_is_pinned_only_when_the_first_covers_and_the_second_does_not() {
        assert_eq!(mark(5, ahead()), Mark::Pinned);
        assert_eq!(mark(5, Lead { first: COVERS_FROM, second: COVERS_FROM - 1e-9 }), Mark::Pinned);
        assert_eq!(mark(5, Lead { first: 1.0, second: 0.0 }), Mark::Pinned);
    }

    #[test]
    fn grade_five_stays_partial_when_the_second_also_covers_or_the_first_does_not() {
        assert_eq!(mark(5, Lead { first: 1.0, second: COVERS_FROM }), Mark::Partial, "the second covers too");
        assert_eq!(mark(5, Lead { first: 1.0, second: 1.0 }), Mark::Partial);
        assert_eq!(mark(5, Lead { first: COVERS_FROM - 1e-9, second: 0.0 }), Mark::Partial, "the first does not cover");
        assert_eq!(mark(5, Lead::default()), Mark::Partial, "no check, no lead");
    }

    #[test]
    fn the_lead_is_the_first_covering_and_the_second_not() {
        assert!(Lead { first: 0.7, second: 0.69 }.leads());
        assert!(!Lead { first: 0.7, second: 0.7 }.leads());
        assert!(!Lead { first: 0.69, second: 0.0 }.leads());
        assert!(!Lead::default().leads());
        assert!((COVERS_FROM - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn a_high_chance_alone_no_longer_pins_the_answer() {
        let lone = signals(1, 1, 9.0, None);
        assert!(grade(&lone) >= 5 && chance(&lone) > 0.93);
        assert_eq!(mark(grade(&lone), Lead::default()), Mark::Partial);
        assert_eq!(mark(grade(&lone), ahead()), Mark::Pinned);
    }

    #[test]
    fn the_mark_keys_are_the_words_of_the_answer() {
        assert_eq!(
            [Mark::Pinned, Mark::Partial, Mark::NotFound].map(Mark::key),
            ["pinned", "partial", "not_found"]
        );
    }

    #[test]
    fn the_not_found_line_carries_the_split_words_the_standard_tools_and_the_exact_search() {
        let words = ["boleto".to_string(), "vencido".to_string()];
        assert_eq!(
            not_found("boletoVencido", &words, Locale::PtBr),
            "Não achei \"boleto\", \"vencido\" no mapa. Siga com suas ferramentas: `Grep`, `Glob` e `Read`. \
             Para começar, busque o texto exato: grep -rniE \"boleto|vencido\" ."
        );
        assert_eq!(
            not_found("boletoVencido", &words, Locale::EnUs),
            "Found nothing for \"boleto\", \"vencido\" in the map. Go on with your tools: `Grep`, `Glob` and `Read`. \
             To start, search the exact text: grep -rniE \"boleto|vencido\" ."
        );
        assert!(not_found("de", &[], Locale::PtBr).contains("grep -rniE \"de\" ."));
    }

    #[test]
    fn a_finding_at_seven_tenths_of_the_first_stays_and_just_below_it_goes() {
        assert!(keeps(7.0, 10.0));
        assert!(keeps(10.0, 10.0));
        assert!(!keeps(6.99, 10.0));
        assert!(!keeps(0.0, 10.0));
    }
}
