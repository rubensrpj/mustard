//! `map_filter` — o filtro da busca por assunto do mapa, como uma tomada.
//!
//! O banco devolve os melhores candidatos na ordem dele; o filtro classifica
//! todos de uma vez contra a frase de quem procura: dá a cada um a chance de
//! ser o que se procura, e uma chance à opção "nenhum destes" — as chances
//! somam 1. Devolve o veredito ([`Verdict`]) e só o que passa do corte. A
//! busca depende só do [`MapFilter`]: a implementação (um serviço pago, um
//! modelo baixado) é escolhida num ponto só, na montagem, e pode ser trocada
//! sem mexer na busca.
//!
//! O veredito ([`verdict`]) e o corte ([`cut`]) são funções puras e moram
//! aqui, para que toda implementação decida do mesmo jeito.
//!
//! Sem disco, sem rede, sem relógio.

use thiserror::Error;

// ---------------------------------------------------------------------------
// O pedido e a resposta
// ---------------------------------------------------------------------------

/// Um candidato do banco, com o que o índice do código sabe dele.
///
/// Só nomes, caminho, assinatura, documentação, comentários, o dono e títulos
/// de commit: o tipo não tem onde pôr uma linha do corpo do código nem um
/// texto entre aspas do corpo, e por isso nenhum dos dois chega ao filtro.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterCandidate {
    /// O id da declaração no banco; volta no que passa do corte.
    pub id: i64,
    /// O tipo da declaração (`function`, `struct`, `method`…).
    pub kind: String,
    /// O nome como está no código; a implementação o quebra em palavras.
    pub name: String,
    /// O caminho do arquivo, relativo à raiz do projeto.
    pub path: String,
    /// A primeira linha da declaração.
    pub line: u32,
    /// A última linha da declaração.
    pub end_line: u32,
    /// A assinatura, como o mapa a guarda.
    pub signature: String,
    /// A documentação, como o mapa a guarda.
    pub documentation: String,
    /// O que contém a declaração (o tipo, o bloco `impl`, a classe), em
    /// texto; a implementação tira dele os nomes.
    pub owner: String,
    /// Os nomes dos membros que um tipo declara, os métodos primeiro, com
    /// `()` no fim. Vazio fora de um tipo.
    pub members: Vec<String>,
    /// O texto dos comentários de dentro do corpo, sem o código.
    pub body_comments: String,
    /// Os títulos dos commits que mudaram o arquivo, do mais novo ao mais
    /// velho.
    pub file_commits: Vec<String>,
}

/// O que a busca pede ao filtro.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FilterRequest {
    /// As palavras de quem procura: nomes do código e termos do pedido, que o
    /// banco usou; podem não existir no código.
    pub words: Vec<String>,
    /// A frase do que se procura e para quê. Vazia, o filtro usa as palavras.
    pub phrase: String,
    /// A parte da maior chance que um candidato precisa ter para passar do
    /// corte (ver [`cut`]), de 0 a 1.
    pub share: f64,
    /// Os candidatos, na ordem do banco.
    pub candidates: Vec<FilterCandidate>,
}

/// Um candidato que passou do corte, com a nota que o filtro deu a ele.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored {
    /// O id do candidato no banco.
    pub id: i64,
    /// A chance de o candidato ser o que se procura, de 0 a 1. Somada à de
    /// todos os outros e à de "nenhum destes", dá 1.
    pub score: f64,
}

/// O que a classificação diz da lista inteira.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// "Nenhum destes" tem a chance de [`NONE_FROM`] ou mais: nada da lista é
    /// o que se procura.
    NotFound,
    /// O classificador tem a confiança de [`SURE_FROM`] ou mais na escolha:
    /// os que passam do corte valem.
    Sure,
    /// O resto: a escolha está dividida entre alguns candidatos.
    Split,
}

/// O que o filtro gastou numa chamada.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterUsage {
    /// Tokens de entrada cobrados, somados de todos os pedidos.
    pub input_tokens: u64,
    /// Milissegundos da chamada inteira, do primeiro pedido à última resposta.
    pub millis: u64,
    /// O custo da chamada em milionésimos de dólar: os tokens cobrados vezes
    /// o preço de tabela do serviço.
    pub cost_micro_usd: u64,
    /// O nome do modelo que respondeu, como a resposta o diz. Vazio quando
    /// ela não diz.
    pub model: String,
}

/// A resposta do filtro: o veredito, o que passou do corte, na ordem da
/// chance, e o uso. Com o veredito de não achei, nada passa do corte.
#[derive(Debug, Clone, PartialEq)]
pub struct Filtered {
    pub verdict: Verdict,
    pub kept: Vec<Scored>,
    pub usage: FilterUsage,
}

/// Por que o filtro não respondeu. Nenhuma mensagem leva a chave nem o corpo
/// da resposta do serviço.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FilterError {
    /// Não há chave: nem no ambiente, nem no `mustard.json` do projeto.
    #[error("no key for the filter service")]
    MissingKey,
    /// A chave está no `mustard.json`, mas o git guarda o arquivo: a chave
    /// dele não se usa, e o aviso pede para tirá-lo do git e trocar a chave.
    #[error("the key in mustard.json is not used, because git tracks the file")]
    KeyInGit,
    /// O pedido não chegou ou a resposta não voltou por falha de rede.
    #[error("network: {0}")]
    Network(String),
    /// O serviço recusou o pedido com este código HTTP.
    #[error("refused with HTTP {status}")]
    Refused { status: u16 },
    /// O serviço não respondeu a tempo.
    #[error("timed out")]
    Timeout,
    /// A resposta veio, mas sem a forma esperada.
    #[error("unreadable response: {0}")]
    Unreadable(String),
    /// O pedido passaria do limite do serviço e não foi mandado.
    #[error("request too large: about {estimated_tokens} tokens")]
    TooLarge { estimated_tokens: u64 },
}

impl FilterError {
    /// O motivo da falha numa palavra só, para o aviso e para o registro da
    /// chamada: nunca leva a chave nem o corpo da resposta.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::MissingKey => "missing_key",
            Self::KeyInGit => "key_in_git",
            Self::Network(_) => "network",
            Self::Refused { status: 401 | 403 } => "key_refused",
            Self::Refused { status: 402 } => "no_credit",
            Self::Refused { status: 429 } => "busy",
            Self::Refused { .. } => "refused",
            Self::Timeout => "timeout",
            Self::Unreadable(_) => "unreadable",
            Self::TooLarge { .. } => "too_large",
        }
    }
}

/// A tomada: quem dá nota aos candidatos e devolve o que passa do corte.
pub trait MapFilter {
    /// Classifica os candidatos de `request` contra a frase e devolve o
    /// veredito e o que passa do [`cut`], na ordem da chance, com o uso da
    /// chamada.
    fn filter(&self, request: &FilterRequest) -> Result<Filtered, FilterError>;
}

// ---------------------------------------------------------------------------
// O veredito e o corte
// ---------------------------------------------------------------------------

/// Quantos itens voltam, no máximo.
pub const MAX_KEPT: usize = 12;

/// A chance de "nenhum destes" a partir da qual a resposta é não achei. A
/// documentação do serviço manda pôr a opção quando a lista pode não ter a
/// resposta; com ela, os 20 pedidos de coisas que o projeto não tem voltaram
/// todos como não achei, e nenhum pedido com o certo na lista voltou assim.
pub const NONE_FROM: f64 = 0.5;

/// A confiança da escolha a partir da qual a classificação vale sem mais
/// nada. Abaixo dela e com "nenhum destes" abaixo de [`NONE_FROM`], a escolha
/// está dividida.
pub const SURE_FROM: f64 = 0.7;

/// Quantos candidatos a segunda olhada relê: os de maior chance da primeira.
pub const FINALISTS: usize = 3;

/// O sim a partir do qual um finalista vale na segunda olhada. Na medida das
/// 120 buscas, com esse corte a segunda olhada pôs o certo em primeiro em
/// 115, e os 20 pedidos de coisas que o projeto não tem já voltaram como não
/// achei na primeira etapa.
pub const YES_FROM: f64 = 0.4;

/// A parte da maior chance que um candidato precisa ter para passar do
/// corte, quando o projeto não diz outra: 0,10. Nas 120 buscas medidas o
/// certo em primeiro foi o mesmo com qualquer corte, e o certo entre os
/// mantidos foi de 116 com 0,5 a 119 com 0,10, com 1,9 item por busca em
/// média; 0,25 ou mais perde o certo em 3 a 4 buscas.
pub const CUT_SHARE: f64 = 0.10;

/// O veredito de uma classificação: "nenhum destes" com [`NONE_FROM`] ou mais
/// de chance é não achei; senão, a confiança de [`SURE_FROM`] ou mais é certo;
/// o resto é dividido.
#[must_use]
pub fn verdict(none: f64, confidence: f64) -> Verdict {
    if none >= NONE_FROM {
        Verdict::NotFound
    } else if confidence >= SURE_FROM {
        Verdict::Sure
    } else {
        Verdict::Split
    }
}

/// O corte: ordena pela chance, da maior para a menor, e no empate fica a
/// ordem de `scores` (a do banco). Fica o melhor e os de chance acima de
/// `share` vezes a maior, até [`MAX_KEPT`]; a chance que só iguala essa linha
/// cai (0,5, 0,45 e 0,05 com 0,10 deixam dois). Sem mínimo e sem piso: as
/// chances somam 1 com a de "nenhum destes", e o que sobra depois do melhor é
/// quase tudo zero.
#[must_use]
pub fn cut(scores: &[Scored], share: f64) -> Vec<Scored> {
    let mut ranked = scores.to_vec();
    // `sort_by` é estável: no empate, a ordem do banco fica.
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
    let Some(best) = ranked.first().map(|s| s.score) else {
        return Vec::new();
    };
    let threshold = best * share;
    ranked
        .into_iter()
        .enumerate()
        .filter(|(at, s)| *at == 0 || s.score > threshold)
        .map(|(_, s)| s)
        .take(MAX_KEPT)
        .collect()
}

/// A classificação inteira posta na resposta do filtro: o veredito de `none`
/// e `confidence` e, salvo no não achei, o que passa do [`cut`] de `share`.
#[must_use]
pub fn judged(scores: &[Scored], none: f64, confidence: f64, share: f64) -> (Verdict, Vec<Scored>) {
    let verdict = verdict(none, confidence);
    let kept = if verdict == Verdict::NotFound { Vec::new() } else { cut(scores, share) };
    (verdict, kept)
}

/// Um finalista da segunda olhada, com o que o serviço respondeu dele.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Finalist {
    /// O id do candidato no banco.
    pub id: i64,
    /// A chance dele na pergunta de escolha entre os finalistas.
    pub chance: f64,
    /// O sim à pergunta "este é o código que se procura?", só dele.
    pub yes: f64,
}

/// Os finalistas da segunda olhada: até [`FINALISTS`] chances de `scores`,
/// da maior para a menor; no empate fica a ordem de `scores` (a do banco).
#[must_use]
pub fn finalists_of(scores: &[Scored]) -> Vec<Scored> {
    let mut ranked = scores.to_vec();
    // `sort_by` é estável: no empate, a ordem do banco fica.
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
    ranked.truncate(FINALISTS);
    ranked
}

/// A decisão da segunda olhada. O finalista de maior sim precisa de
/// [`YES_FROM`] ou mais; sem ele, é não achei e nada volta. Havendo, entrega
/// o vencedor da escolha entre os finalistas, quando o sim dele também chega
/// a [`YES_FROM`]; se o vencedor for "nenhum destes" (`none` maior que a
/// chance de todos) ou o sim dele ficar abaixo, entrega o de maior sim. O
/// que volta é um só, com o sim dele como nota, e o veredito é certo.
#[must_use]
pub fn second_look(finalists: &[Finalist], none: f64) -> (Verdict, Vec<Scored>) {
    // `max_by` devolve o último dos iguais; no empate vale o primeiro.
    let first_of = |key: fn(&Finalist) -> f64| {
        finalists.iter().rev().max_by(|a, b| key(a).total_cmp(&key(b))).copied()
    };
    let Some(surest) = first_of(|f| f.yes).filter(|f| f.yes >= YES_FROM) else {
        return (Verdict::NotFound, Vec::new());
    };
    let winner = first_of(|f| f.chance).filter(|f| f.chance >= none && f.yes >= YES_FROM);
    let chosen = winner.unwrap_or(surest);
    (Verdict::Sure, vec![Scored { id: chosen.id, score: chosen.yes }])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finalist(id: i64, chance: f64, yes: f64) -> Finalist {
        Finalist { id, chance, yes }
    }

    fn delivered(finalists: &[Finalist], none: f64) -> (Verdict, Vec<i64>) {
        let (verdict, kept) = second_look(finalists, none);
        (verdict, kept.iter().map(|s| s.id).collect())
    }

    #[test]
    fn the_finalists_are_the_three_biggest_chances_and_the_bank_order_breaks_ties() {
        let top = finalists_of(&scores(&[0.1, 0.4, 0.2, 0.4, 0.3]));
        assert_eq!(ids(&top), vec![2, 4, 5]);
        assert_eq!(ids(&finalists_of(&scores(&[0.6, 0.4]))), vec![1, 2], "fewer than three stay whole");
        assert!(finalists_of(&[]).is_empty());
    }

    #[test]
    fn the_winner_of_the_choice_is_delivered_when_its_yes_is_four_tenths_or_more() {
        // O vencedor da escolha (id 1) tem sim 0,4 e o outro tem 0,95: fica o vencedor.
        let both = [finalist(1, 0.6, 0.4), finalist(2, 0.3, 0.95), finalist(3, 0.05, 0.1)];
        assert_eq!(delivered(&both, 0.05), (Verdict::Sure, vec![1]));
        let (_, kept) = second_look(&both, 0.05);
        assert!((kept[0].score - 0.4).abs() < f64::EPSILON, "the note is the yes of the delivered one");
    }

    #[test]
    fn a_winner_below_four_tenths_gives_way_to_the_highest_yes() {
        let below = [finalist(1, 0.6, 0.399_999), finalist(2, 0.3, 0.95), finalist(3, 0.05, 0.6)];
        assert_eq!(delivered(&below, 0.05), (Verdict::Sure, vec![2]));
    }

    #[test]
    fn every_yes_below_four_tenths_is_not_found_and_keeps_nothing() {
        let all = [finalist(1, 0.6, 0.39), finalist(2, 0.3, 0.2), finalist(3, 0.05, 0.0)];
        assert_eq!(second_look(&all, 0.05), (Verdict::NotFound, Vec::new()));
        assert_eq!(second_look(&[], 0.5), (Verdict::NotFound, Vec::new()));
    }

    #[test]
    fn none_winning_the_choice_leaves_the_highest_yes_to_deliver() {
        // "Nenhum destes" tem mais chance que todos: o vencedor não é finalista.
        let none_wins = [finalist(1, 0.2, 0.5), finalist(2, 0.1, 0.8)];
        assert_eq!(delivered(&none_wins, 0.7), (Verdict::Sure, vec![2]));
        // Empatada com a melhor chance, a escolha é do finalista.
        assert_eq!(delivered(&[finalist(1, 0.4, 0.5), finalist(2, 0.2, 0.8)], 0.4), (Verdict::Sure, vec![1]));
    }

    #[test]
    fn equal_chances_and_equal_yes_answers_keep_the_first_in_the_order_given() {
        let tied = [finalist(1, 0.3, 0.7), finalist(2, 0.3, 0.7), finalist(3, 0.3, 0.7)];
        assert_eq!(delivered(&tied, 0.1), (Verdict::Sure, vec![1]));
        let by_yes = [finalist(1, 0.1, 0.9), finalist(2, 0.5, 0.2), finalist(3, 0.2, 0.9)];
        assert_eq!(delivered(&by_yes, 0.1), (Verdict::Sure, vec![1]), "the winner has no yes, so the first of the highest");
    }

    /// As chances na ordem do banco, com os ids 1, 2, 3…
    fn scores(notes: &[f64]) -> Vec<Scored> {
        notes.iter().enumerate().map(|(i, &score)| Scored { id: i as i64 + 1, score }).collect()
    }

    fn ids(kept: &[Scored]) -> Vec<i64> {
        kept.iter().map(|s| s.id).collect()
    }

    #[test]
    fn a_concentrated_chance_keeps_only_the_best() {
        // 0,9 × 0,10 = 0,09: só o 0,9 passa; 0,05 e 0,03 caem.
        let kept = cut(&scores(&[0.9, 0.05, 0.03]), CUT_SHARE);
        assert_eq!(ids(&kept), vec![1]);
        assert!((kept[0].score - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn a_split_chance_keeps_the_two_close_ones() {
        // 0,5 × 0,10 = 0,05: ficam 0,5 e 0,45; o 0,05 só iguala a linha e cai.
        assert_eq!(ids(&cut(&scores(&[0.5, 0.45, 0.05]), CUT_SHARE)), vec![1, 2]);
        assert_eq!(ids(&cut(&scores(&[0.5, 0.45, 0.04]), CUT_SHARE)), vec![1, 2]);
    }

    #[test]
    fn the_cut_line_is_the_share_of_the_best_and_only_what_passes_it_stays() {
        // A divisa: a chance igual à parte da maior cai; um fio acima, fica.
        assert_eq!(ids(&cut(&scores(&[0.5, 0.45, 0.05 + 1e-9]), CUT_SHARE)), vec![1, 2, 3]);
        assert_eq!(ids(&cut(&scores(&[0.5, 0.45, 0.05]), CUT_SHARE)), vec![1, 2]);
    }

    #[test]
    fn the_best_stays_even_when_every_chance_is_zero() {
        assert_eq!(ids(&cut(&scores(&[0.0, 0.0]), CUT_SHARE)), vec![1]);
    }

    #[test]
    fn the_cut_has_no_minimum_and_no_floor() {
        // Tudo abaixo de 0,15 e nenhum mínimo: volta o melhor e só ele.
        let kept = cut(&scores(&[0.12, 0.011, 0.01]), CUT_SHARE);
        assert_eq!(ids(&kept), vec![1]);
    }

    #[test]
    fn the_cut_returns_at_most_twelve() {
        let kept = cut(&scores(&[0.08; 15]), CUT_SHARE);
        assert_eq!(ids(&kept), (1..=12).collect::<Vec<_>>());
    }

    #[test]
    fn the_bank_order_breaks_ties() {
        let kept = cut(&scores(&[0.3, 0.4, 0.3, 0.4]), CUT_SHARE);
        assert_eq!(ids(&kept), vec![2, 4, 1, 3]);
    }

    #[test]
    fn a_bigger_share_cuts_closer_to_the_best() {
        let chances = scores(&[0.6, 0.25, 0.1]);
        assert_eq!(ids(&cut(&chances, 0.10)), vec![1, 2, 3]);
        assert_eq!(ids(&cut(&chances, 0.5)), vec![1]);
    }

    #[test]
    fn nothing_to_cut_returns_nothing() {
        assert!(cut(&[], CUT_SHARE).is_empty());
    }

    #[test]
    fn none_at_half_is_not_found_whatever_the_confidence() {
        assert_eq!(verdict(0.5, 0.99), Verdict::NotFound);
        assert_eq!(verdict(0.6, 0.2), Verdict::NotFound);
        assert_eq!(verdict(0.499_999, 0.99), Verdict::Sure);
    }

    #[test]
    fn a_confidence_of_seven_tenths_is_sure_and_below_it_is_split() {
        assert_eq!(verdict(0.1, 0.7), Verdict::Sure);
        assert_eq!(verdict(0.1, 0.699_999), Verdict::Split);
        assert_eq!(verdict(0.0, 0.0), Verdict::Split);
    }

    #[test]
    fn a_not_found_verdict_keeps_nothing_and_the_others_keep_the_cut() {
        let chances = scores(&[0.4, 0.1, 0.05]);
        let (verdict, kept) = judged(&chances, 0.6, 0.9, CUT_SHARE);
        assert_eq!((verdict, kept), (Verdict::NotFound, Vec::new()));
        let (verdict, kept) = judged(&chances, 0.3, 0.9, CUT_SHARE);
        assert_eq!((verdict, ids(&kept)), (Verdict::Sure, vec![1, 2, 3]));
        let (verdict, kept) = judged(&chances, 0.3, 0.4, 0.5);
        assert_eq!((verdict, ids(&kept)), (Verdict::Split, vec![1]));
    }
}
