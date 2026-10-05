//! A medição do uso real: o gasto do Claude num projeto antes e depois de uma
//! versão do Mustard chegar a ele. A conta pura, sem disco e sem relógio.
//!
//! A marca é a primeira sessão de uma compilação do Mustard no projeto. Os
//! dois lados se comparam em dias contados — dias fechados com
//! [`MIN_ACTIONS`] ações ou mais —, pela mesma soma da página do gasto. O dia
//! da marca fica fora dos dois lados: nele as duas versões se misturam.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::spend::{day_of, totals, DayRow, DayTotal, MIN_ACTIONS};

/// O mínimo de dias contados de cada lado para a medição afirmar.
pub const MIN_DAYS: usize = 5;

/// A marca de uma versão no projeto: a linha de versão do programa (a versão
/// e o carimbo da compilação, porque a versão em desenvolvimento repete o
/// número) e o instante, em RFC 3339, da primeira sessão com ela.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub version: String,
    pub at: String,
}

/// Um lado da comparação: os dias contados, do mais velho ao mais novo, e a
/// soma deles.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Side {
    pub days: Vec<String>,
    pub actions: u64,
    pub tokens: u64,
    /// Os tokens divididos pelas ações, arredondado; zero sem dia contado.
    pub tokens_per_action: u64,
}

/// O que a medição pode dizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Verdict {
    /// Nenhum dia contado depois da marca: a versão ainda não foi usada no
    /// projeto, e o antes já tem a janela de [`MIN_DAYS`] dias.
    NotUsedYet,
    /// Um dos lados tem menos de [`MIN_DAYS`] dias contados: ainda não dá
    /// para dizer. Quantos dias contados faltam a cada lado.
    TooEarly { missing_before: usize, missing_after: usize },
    /// Os dois lados com o mínimo: a comparação vale.
    Ready,
}

/// Os dois lados e o veredito.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Measurement {
    pub before: Side,
    pub after: Side,
    pub verdict: Verdict,
}

/// A soma dos dias de um lado.
fn side(days: &[&DayTotal]) -> Side {
    let actions = days.iter().fold(0, |sum: u64, day| sum.saturating_add(day.actions));
    let tokens = days.iter().fold(0, |sum: u64, day| sum.saturating_add(day.tokens));
    Side {
        days: days.iter().map(|day| day.day.clone()).collect(),
        actions,
        tokens,
        tokens_per_action: tokens.saturating_add(actions / 2).checked_div(actions).unwrap_or(0),
    }
}

/// O antes e o depois da marca feita no instante `mark`, nas linhas de dia
/// `rows` de um projeto, quando hoje é `today`. Hoje é aberto e não conta.
///
/// Depois: os dias contados desde a marca. Antes: a mesma quantidade de dias
/// contados, os mais novos antes da marca; sem dia depois, a janela de
/// [`MIN_DAYS`]. Abaixo do mínimo de um dos lados, os números que existem
/// saem com o veredito de que ainda não dá para dizer.
#[must_use]
pub fn measure(rows: &[DayRow], mark: DateTime<Utc>, today: &str) -> Measurement {
    let mark_day = day_of(mark);
    let counted: Vec<DayTotal> = totals(rows)
        .into_iter()
        .filter(|day| day.day.as_str() < today && day.actions >= MIN_ACTIONS)
        .collect();
    let after: Vec<&DayTotal> = counted.iter().filter(|day| day.day > mark_day).collect();
    let window = if after.is_empty() { MIN_DAYS } else { after.len() };
    let mut before: Vec<&DayTotal> = counted.iter().rev().filter(|day| day.day < mark_day).take(window).collect();
    before.reverse();
    let missing = |days: &[&DayTotal]| MIN_DAYS.saturating_sub(days.len());
    let verdict = match (missing(&before), missing(&after)) {
        _ if after.is_empty() => Verdict::NotUsedYet,
        (0, 0) => Verdict::Ready,
        (missing_before, missing_after) => Verdict::TooEarly { missing_before, missing_after },
    };
    Measurement { before: side(&before), after: side(&after), verdict }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um caso da tabela: o nome, as linhas, a marca, hoje, os dias de antes,
    /// os de depois e o veredito.
    type Case<'a> = (&'a str, &'a [DayRow], &'a str, &'a str, &'a [&'a str], &'a [&'a str], Verdict);

    /// Os dias de 25 a 30 de setembro: o dia 25 com 100 ações conta, o 26 com
    /// 99 não conta.
    const SEPTEMBER: [(&str, u64, u64); 6] = [
        ("2026-09-25", 100, 100_000),
        ("2026-09-26", 99, 99_000),
        ("2026-09-27", 1_000, 1_000_000),
        ("2026-09-28", 1_000, 1_000_000),
        ("2026-09-29", 1_000, 1_000_000),
        ("2026-09-30", 1_000, 1_000_000),
    ];

    /// O dia da marca e o de hoje ficam fora dos dois lados, e o dia fraco
    /// também; o antes tem o tamanho do depois, com os dias mais novos antes
    /// da marca; abaixo de cinco dias de um lado a conta diz quantos faltam a
    /// cada um; sem dia depois, a versão ainda não foi usada e o antes leva a
    /// janela de cinco dias.
    #[test]
    fn the_sides_and_the_verdict_follow_the_counted_days_around_the_mark() {
        // As linhas de um projeto, cada uma um dia com as ações e os tokens dele.
        let rows = |days: &[(&str, u64, u64)]| -> Vec<DayRow> {
            days.iter()
                .map(|&(day, actions, tokens)| DayRow { day: day.into(), project: "p".into(), tokens, actions, ..DayRow::default() })
                .collect()
        };
        let at = |stamp: &str| DateTime::parse_from_rfc3339(stamp).unwrap().with_timezone(&Utc);
        let model = rows(&[
            ("2026-09-27", 5_000, 600_000_000),
            ("2026-09-28", 50, 9_000_000),
            ("2026-09-29", 7_000, 900_000_000),
            ("2026-09-30", 7_222, 877_000_000),
            ("2026-10-01", 9_000, 999_000_000),
            ("2026-10-02", 4_000, 600_000_000),
            ("2026-10-03", 4_663, 630_000_000),
            ("2026-10-04", 9_000, 999_000_000),
        ]);
        let after = |days: &[&'static str]| days.iter().map(|day| (*day, 1_000, 1_000_000)).collect::<Vec<_>>();
        let full = rows(&[SEPTEMBER.as_slice(), &after(&["2026-10-02", "2026-10-03", "2026-10-04", "2026-10-05", "2026-10-06"])].concat());
        let short = rows(&[&SEPTEMBER[4..], &after(&["2026-10-02", "2026-10-03", "2026-10-04", "2026-10-05", "2026-10-06", "2026-10-07"])].concat());
        let five_before = ["2026-09-25", "2026-09-27", "2026-09-28", "2026-09-29", "2026-09-30"];
        let five_after = ["2026-10-02", "2026-10-03", "2026-10-04", "2026-10-05", "2026-10-06"];
        let cases: [Case; 4] = [
            (
                // 21h03 de 1º de outubro em -03:00 já é 2 de outubro em UTC.
                "two days on each side",
                &model,
                "2026-10-01T21:03:00-03:00",
                "2026-10-04",
                &["2026-09-29", "2026-09-30"],
                &["2026-10-02", "2026-10-03"],
                Verdict::TooEarly { missing_before: 3, missing_after: 3 },
            ),
            ("five on each side", &full, "2026-10-01T10:00:00-03:00", "2026-10-07", &five_before, &five_after, Verdict::Ready),
            ("no day after", &full, "2026-10-01T10:00:00-03:00", "2026-10-02", &five_before, &[], Verdict::NotUsedYet),
            (
                "a short history before",
                &short,
                "2026-10-01T10:00:00-03:00",
                "2026-10-08",
                &["2026-09-29", "2026-09-30"],
                &["2026-10-02", "2026-10-03", "2026-10-04", "2026-10-05", "2026-10-06", "2026-10-07"],
                Verdict::TooEarly { missing_before: 3, missing_after: 0 },
            ),
        ];
        for (name, rows, mark, today, before, after, verdict) in cases {
            let measured = measure(rows, at(mark), today);
            assert_eq!(measured.before.days, before, "{name}: before");
            assert_eq!(measured.after.days, after, "{name}: after");
            assert_eq!(measured.verdict, verdict, "{name}: verdict");
        }

        let measured = measure(&model, at("2026-10-01T21:03:00-03:00"), "2026-10-04");
        let numbers = |side: &Side| (side.actions, side.tokens, side.tokens_per_action);
        assert_eq!(numbers(&measured.before), (14_222, 1_777_000_000, 124_947), "125 thousand per action before");
        assert_eq!(numbers(&measured.after), (8_663, 1_230_000_000, 141_983), "142 thousand per action after");
        let unused = measure(&full, at("2026-10-01T10:00:00-03:00"), "2026-10-02");
        assert_eq!((unused.after.actions, unused.after.tokens_per_action), (0, 0), "no use after the mark");
    }
}
