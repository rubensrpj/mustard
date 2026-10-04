//! `map_filter` — o filtro da busca por assunto do mapa, como uma tomada.
//!
//! O banco devolve todos os candidatos que achou, na ordem dele; o filtro lê
//! todos contra o que o agente queria e responde duas coisas: a chance de
//! cada candidato ser o código que se procura (as chances somam 1) e a chance
//! de algum deles ser. Devolve o veredito ([`Verdict`]) e só o que passa do
//! corte ([`CutRule`]). A busca depende só do [`MapFilter`]: a implementação
//! (um serviço pago, um modelo baixado) é escolhida num ponto só, na
//! montagem, e pode ser trocada sem mexer na busca.
//!
//! Quando a lista não cabe num pedido só, a implementação a divide em vários
//! e junta as respostas ([`joined`]). O veredito, o corte e a junção são
//! funções puras ([`judged`], [`cut`], [`joined`]) e moram aqui, para que toda
//! implementação decida do mesmo jeito.
//!
//! Sem disco, sem rede, sem relógio.

use std::path::PathBuf;

use thiserror::Error;

// ---------------------------------------------------------------------------
// O pedido e a resposta
// ---------------------------------------------------------------------------

/// Um candidato do banco, com o que o índice do código sabe dele.
///
/// O tipo não guarda uma linha do corpo do código: o filtro que quer o código
/// lê o arquivo do candidato, pelo caminho e pelas linhas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterCandidate {
    /// O id da declaração no banco; volta no que passa do corte.
    pub id: i64,
    /// O tipo da declaração (`function`, `struct`, `method`…).
    pub kind: String,
    /// O nome como está no código.
    pub name: String,
    /// O caminho do arquivo, relativo à raiz do projeto.
    pub path: String,
    /// A primeira linha da declaração.
    pub line: u32,
    /// A última linha da declaração.
    pub end_line: u32,
    /// A assinatura, como o mapa a guarda.
    pub signature: String,
    /// A documentação inteira, como o mapa a guarda.
    pub documentation: String,
    /// Os títulos de todos os commits que mudaram a declaração, do mais novo
    /// ao mais velho; vazio até quem monta o pedido ler a história do mapa.
    pub commits: Vec<String>,
    /// Os comentários de revisão presos às linhas da declaração, inteiros;
    /// vazio até quem monta o pedido ler a história do mapa.
    pub reviews: Vec<String>,
    /// Os nomes das declarações do projeto que ela chama, como o grafo do
    /// mapa os guarda.
    pub calls: Vec<String>,
}

/// As duas regras do corte, em números: a parte da maior chance que um
/// candidato precisa ter e o sim a partir do qual algum candidato existe. São
/// o veredito do filtro, e não um teto de quantidade: passa tudo o que chega
/// à linha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutRule {
    /// A parte da maior chance que um candidato precisa ter para passar do
    /// corte, de 0 a 1.
    pub share: f64,
    /// A chance de algum candidato ser o que se procura, de 0 a 1, a partir
    /// da qual há resposta; abaixo dela é não achei.
    pub exists_from: f64,
}

impl Default for CutRule {
    /// Os números medidos: [`CUT_SHARE`] e [`EXISTS_FROM`].
    fn default() -> Self {
        Self { share: CUT_SHARE, exists_from: EXISTS_FROM }
    }
}

/// O que a busca pede ao filtro.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FilterRequest {
    /// As palavras de quem procura: nomes do código e termos do pedido, que o
    /// banco usou; podem não existir no código.
    pub words: Vec<String>,
    /// A frase do que se procura e para quê. Vazia, o filtro usa as palavras.
    pub phrase: String,
    /// A descrição que o agente deu à busca; vazia quando ele não deu.
    pub described: String,
    /// A última fala do agente antes da busca; vazia quando não há. Nunca é
    /// texto do usuário.
    pub said: String,
    /// A raiz do projeto, de onde o filtro lê o código de cada candidato.
    pub root: PathBuf,
    /// O corte da resposta.
    pub cut: CutRule,
    /// Os candidatos, na ordem do banco.
    pub candidates: Vec<FilterCandidate>,
}

/// Um candidato que passou do corte, com a nota que o filtro deu a ele.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored {
    /// O id do candidato no banco.
    pub id: i64,
    /// A chance de o candidato ser o que se procura, de 0 a 1. Somada à de
    /// todos os outros, dá 1.
    pub score: f64,
}

/// O que a classificação diz da lista inteira.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// A chance de algum candidato ser o que se procura ficou abaixo de
    /// [`EXISTS_FROM`]: nada da lista serve.
    NotFound,
    /// Algum candidato serve: os que passam do corte valem.
    Found,
}

/// O que o filtro gastou numa chamada, a soma de todos os pedidos dela.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterUsage {
    /// Tokens de entrada cobrados.
    pub input_tokens: u64,
    /// Milissegundos da chamada inteira, do pedido à resposta.
    pub millis: u64,
    /// O custo da chamada em milionésimos de dólar: os tokens cobrados vezes
    /// o preço de tabela do serviço.
    pub cost_micro_usd: u64,
    /// Quantos pedidos ao serviço a chamada fez: um por lista que coube num
    /// pedido, e os do candidato grande demais, cada um com o seu.
    pub requests: u64,
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
    /// O gasto do mês mais o pedido passaria do teto de gasto do serviço, e
    /// o pedido não foi mandado.
    #[error("the spend of the month would pass the budget")]
    OverBudget,
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
            Self::OverBudget => "over_budget",
        }
    }
}

/// A tomada: quem dá nota aos candidatos e devolve o que passa do corte.
pub trait MapFilter {
    /// Lê os candidatos de `request` contra o que o agente queria e devolve o
    /// veredito e o que passa do [`cut`], na ordem da chance, com o uso da
    /// chamada.
    fn filter(&self, request: &FilterRequest) -> Result<Filtered, FilterError>;
}

// ---------------------------------------------------------------------------
// O veredito e o corte
// ---------------------------------------------------------------------------

/// A chance de algum candidato ser o que se procura a partir da qual a
/// resposta vale, quando o projeto não diz outra: 0,50. Abaixo dela é não
/// achei. Medido nas mesmas 100 buscas, e conferido em outras 337.
pub const EXISTS_FROM: f64 = 0.50;

/// A parte da maior chance que um candidato precisa ter para passar do
/// corte, quando o projeto não diz outra: 0,10.
pub const CUT_SHARE: f64 = 0.10;

/// O corte: ordena pela chance, da maior para a menor, e no empate fica a
/// ordem de `scores` (a do banco). Fica o melhor, sempre, e todos os de
/// chance de pelo menos `rule.share` vezes a maior, sem teto de quantidade.
/// Sem mínimo e sem piso: as chances somam 1, e o que sobra depois do melhor é
/// quase tudo zero; uma chance zero nunca entra atrás do melhor.
#[must_use]
pub fn cut(scores: &[Scored], rule: CutRule) -> Vec<Scored> {
    let mut ranked = scores.to_vec();
    // `sort_by` é estável: no empate, a ordem do banco fica.
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
    let Some(best) = ranked.first().map(|s| s.score) else {
        return Vec::new();
    };
    let threshold = best * rule.share;
    ranked
        .into_iter()
        .enumerate()
        .filter(|(at, s)| *at == 0 || (s.score > 0.0 && s.score >= threshold))
        .map(|(_, s)| s)
        .collect()
}

/// A classificação inteira posta na resposta do filtro: com a chance de
/// `exists` abaixo de `rule.exists_from`, é não achei e nada passa; senão, o
/// que passa do [`cut`].
#[must_use]
pub fn judged(scores: &[Scored], exists: f64, rule: CutRule) -> (Verdict, Vec<Scored>) {
    if exists < rule.exists_from {
        (Verdict::NotFound, Vec::new())
    } else {
        (Verdict::Found, cut(scores, rule))
    }
}

/// A resposta de um dos pedidos em que a lista de candidatos foi dividida: a
/// chance de algum candidato dele ser o que se procura e a chance de cada um
/// deles ser o escolhido entre os do pedido.
#[derive(Debug, Clone, PartialEq)]
pub struct Partial {
    /// A chance de algum candidato deste pedido ser o que se procura.
    pub exists: f64,
    /// A chance de cada candidato do pedido ser o escolhido entre os dele.
    pub scores: Vec<Scored>,
}

/// As respostas dos pedidos juntas numa só: a nota de cada candidato é a
/// chance de existir do pedido dele vezes a nota dele na escolha, e a chance
/// de existir da lista é a maior das dos pedidos. Os candidatos saem na ordem
/// em que aparecem nos pedidos. O candidato que se repete, o grande demais
/// que vai em vários pedidos seguidos, fica com a maior das notas. Sem
/// pedido, a chance de existir é zero e não há nota.
#[must_use]
pub fn joined(partials: &[Partial]) -> (f64, Vec<Scored>) {
    let mut notes: Vec<Scored> = Vec::new();
    let mut at: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
    for partial in partials {
        for scored in &partial.scores {
            let note = partial.exists * scored.score;
            match at.get(&scored.id) {
                Some(&index) => notes[index].score = notes[index].score.max(note),
                None => {
                    at.insert(scored.id, notes.len());
                    notes.push(Scored { id: scored.id, score: note });
                }
            }
        }
    }
    (partials.iter().map(|partial| partial.exists).fold(0.0, f64::max), notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As chances na ordem do banco, com os ids 1, 2, 3…
    fn scores(notes: &[f64]) -> Vec<Scored> {
        notes.iter().enumerate().map(|(i, &score)| Scored { id: i as i64 + 1, score }).collect()
    }

    fn ids(kept: &[Scored]) -> Vec<i64> {
        kept.iter().map(|s| s.id).collect()
    }

    /// O corte medido, com `share` no lugar da parte da maior chance.
    fn sharing(share: f64) -> CutRule {
        CutRule { share, ..CutRule::default() }
    }

    #[test]
    fn the_measured_numbers_are_the_default_rule() {
        let rule = CutRule::default();
        assert_eq!((rule.share, rule.exists_from), (0.10, 0.50));
    }

    #[test]
    fn a_concentrated_chance_keeps_only_the_best() {
        // 0,9 × 0,10 = 0,09: só o 0,9 passa; 0,05 e 0,03 caem.
        let kept = cut(&scores(&[0.9, 0.05, 0.03]), CutRule::default());
        assert_eq!(ids(&kept), vec![1]);
        assert!((kept[0].score - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn a_split_chance_keeps_the_two_close_ones() {
        assert_eq!(ids(&cut(&scores(&[0.5, 0.45, 0.04]), CutRule::default())), vec![1, 2]);
    }

    #[test]
    fn the_chance_that_only_equals_the_cut_line_stays_in() {
        // 0,5 × 0,10 = 0,05: a chance de 0,05 chega à linha e fica; um fio
        // abaixo, cai.
        let rule = CutRule::default();
        assert_eq!(ids(&cut(&scores(&[0.5, 0.45, 0.05]), rule)), vec![1, 2, 3]);
        assert_eq!(ids(&cut(&scores(&[0.5, 0.45, 0.05 - 1e-9]), rule)), vec![1, 2]);
    }

    #[test]
    fn every_candidate_that_reaches_the_cut_line_passes_with_no_ceiling_on_the_count() {
        let five = ids(&cut(&scores(&[0.2; 5]), CutRule::default()));
        assert_eq!(five, vec![1, 2, 3, 4, 5], "five equal chances above the line all stay");
        let many = cut(&scores(&[0.008; 150]), CutRule::default());
        assert_eq!(many.len(), 150, "a hundred and fifty equal chances all stay");
        let beyond_the_line = ids(&cut(&scores(&[0.4, 0.4, 0.4, 0.4, 0.039]), CutRule::default()));
        assert_eq!(beyond_the_line, vec![1, 2, 3, 4], "only the line cuts: 0,039 is under 0,4 × 0,10");
    }

    #[test]
    fn the_best_stays_even_when_every_chance_is_zero_and_zero_never_follows_it() {
        assert_eq!(ids(&cut(&scores(&[0.0, 0.0]), CutRule::default())), vec![1]);
        assert_eq!(ids(&cut(&scores(&[0.0, 0.0, 0.0]), CutRule::default())), vec![1]);
    }

    #[test]
    fn the_cut_has_no_minimum_and_no_floor() {
        // Tudo abaixo de 0,15 e nenhum mínimo: volta o melhor e só ele.
        let kept = cut(&scores(&[0.12, 0.011, 0.01]), CutRule::default());
        assert_eq!(ids(&kept), vec![1]);
    }

    #[test]
    fn the_bank_order_breaks_ties() {
        let kept = cut(&scores(&[0.3, 0.4, 0.3, 0.4]), CutRule::default());
        assert_eq!(ids(&kept), vec![2, 4, 1, 3], "the equal chances keep the order of the bank");
    }

    #[test]
    fn a_bigger_share_cuts_closer_to_the_best() {
        let chances = scores(&[0.6, 0.25, 0.1]);
        let wide = sharing(0.10);
        assert_eq!(ids(&cut(&chances, wide)), vec![1, 2, 3]);
        assert_eq!(ids(&cut(&chances, CutRule { share: 0.5, ..wide })), vec![1]);
    }

    #[test]
    fn nothing_to_cut_returns_nothing() {
        assert!(cut(&[], CutRule::default()).is_empty());
    }

    #[test]
    fn the_chance_that_one_exists_below_half_is_not_found_and_keeps_nothing() {
        let chances = scores(&[0.9, 0.05, 0.05]);
        assert_eq!(judged(&chances, 0.49, CutRule::default()), (Verdict::NotFound, Vec::new()));
        assert_eq!(judged(&chances, 0.0, CutRule::default()), (Verdict::NotFound, Vec::new()));
        let (verdict, kept) = judged(&chances, 0.50, CutRule::default());
        assert_eq!((verdict, ids(&kept)), (Verdict::Found, vec![1]));
    }

    #[test]
    fn the_exists_line_of_the_rule_moves_the_not_found_line() {
        let chances = scores(&[0.9, 0.05]);
        let strict = CutRule { exists_from: 0.8, ..CutRule::default() };
        assert_eq!(judged(&chances, 0.79, strict).0, Verdict::NotFound);
        assert_eq!(judged(&chances, 0.8, strict).0, Verdict::Found);
        let lax = CutRule { exists_from: 0.2, ..CutRule::default() };
        assert_eq!(judged(&chances, 0.2, lax).0, Verdict::Found);
    }

    #[test]
    fn a_found_verdict_keeps_the_cut_of_the_rule() {
        let chances = scores(&[0.4, 0.1, 0.05]);
        let (verdict, kept) = judged(&chances, 0.9, CutRule::default());
        assert_eq!((verdict, ids(&kept)), (Verdict::Found, vec![1, 2, 3]));
        let (verdict, kept) = judged(&chances, 0.9, CutRule { share: 0.5, ..CutRule::default() });
        assert_eq!((verdict, ids(&kept)), (Verdict::Found, vec![1]));
    }

    /// Um pedido do filtro dividido: a chance de existir e a nota dos
    /// candidatos de `ids`, um a um.
    fn partial(exists: f64, notes: &[(i64, f64)]) -> Partial {
        Partial { exists, scores: notes.iter().map(|&(id, score)| Scored { id, score }).collect() }
    }

    #[test]
    fn the_note_of_each_candidate_is_the_existence_of_its_request_times_its_own_choice_note() {
        // O pedido A existe com 0,9 e escolhe o 3 com 0,8; o B existe com 0,2.
        let a = partial(0.9, &[(1, 0.1), (2, 0.1), (3, 0.8)]);
        let b = partial(0.2, &[(4, 0.3), (5, 0.3), (6, 0.2), (8, 0.2)]);
        let (exists, notes) = joined(&[a, b]);
        assert!((exists - 0.9).abs() < 1e-6, "the existence of the list is the highest of the requests");
        let note = |id: i64| notes.iter().find(|scored| scored.id == id).unwrap().score;
        assert!((note(3) - 0.72).abs() < 1e-9, "0,9 × 0,8");
        assert!((note(4) - 0.06).abs() < 1e-9, "0,2 × 0,3");
        assert_eq!(ids(&notes), vec![1, 2, 3, 4, 5, 6, 8], "the candidates keep the order of the requests");
        let (verdict, kept) = judged(&notes, exists, CutRule::default());
        assert_eq!(
            (verdict, ids(&kept)),
            (Verdict::Found, vec![3, 1, 2]),
            "the best joined note is the 3 (0,72); the 1 and the 2 (0,09) reach its line of 0,072; the 0,06 of the request that barely exists does not"
        );
    }

    #[test]
    fn no_request_reaching_the_existence_line_is_not_found() {
        let a = partial(0.49, &[(1, 0.9), (2, 0.1)]);
        let b = partial(0.2, &[(3, 1.0)]);
        let (exists, notes) = joined(&[a, b]);
        assert!((exists - 0.49).abs() < 1e-6);
        assert_eq!(judged(&notes, exists, CutRule::default()), (Verdict::NotFound, Vec::new()));
        let at_the_line = partial(0.50, &[(1, 0.9), (2, 0.1)]);
        let (exists, notes) = joined(&[at_the_line, partial(0.2, &[(3, 1.0)])]);
        assert_eq!(judged(&notes, exists, CutRule::default()).0, Verdict::Found);
    }

    #[test]
    fn a_candidate_split_in_several_requests_keeps_the_highest_of_its_notes() {
        let parts = [partial(0.1, &[(7, 1.0)]), partial(0.95, &[(7, 1.0)]), partial(0.3, &[(7, 1.0)])];
        let (exists, notes) = joined(&parts);
        assert!((exists - 0.95).abs() < 1e-6);
        assert_eq!(notes, vec![Scored { id: 7, score: 0.95 }]);
    }

    #[test]
    fn without_a_request_nothing_exists_and_nothing_has_a_note() {
        assert_eq!(joined(&[]), (0.0, Vec::new()));
    }
}
