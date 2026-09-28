//! `map_filter` — o filtro da busca por assunto do mapa, como uma tomada.
//!
//! O banco devolve os melhores candidatos na ordem dele; o filtro dá a cada um
//! uma nota contra a frase de quem procura e devolve só o que passa do corte.
//! A busca depende só do [`MapFilter`]: a implementação (um serviço pago, um
//! modelo baixado) é escolhida num ponto só, na montagem, e pode ser trocada
//! sem mexer na busca.
//!
//! O corte ([`cut`]) é função pura e mora aqui, para que toda implementação
//! corte do mesmo jeito.
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterRequest {
    /// As palavras de quem procura: nomes do código e termos do pedido, que o
    /// banco usou; podem não existir no código.
    pub words: Vec<String>,
    /// A frase do que se procura e para quê. Vazia, o filtro usa as palavras.
    pub phrase: String,
    /// Quantos itens no mínimo voltam, quando há candidatos acima do piso
    /// (ver [`cut`]). Zero, vale só o corte.
    pub minimum: usize,
    /// Os candidatos, na ordem do banco.
    pub candidates: Vec<FilterCandidate>,
}

/// Um candidato que passou do corte, com a nota que o filtro deu a ele.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored {
    /// O id do candidato no banco.
    pub id: i64,
    /// A chance de o candidato ser o que se procura, de 0 a 1.
    pub score: f64,
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

/// A resposta do filtro: o que passou do corte, na ordem da nota, e o uso.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filtered {
    pub kept: Vec<Scored>,
    pub usage: FilterUsage,
}

/// Por que o filtro não respondeu. Nenhuma mensagem leva a chave nem o corpo
/// da resposta do serviço.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FilterError {
    /// Não há chave na máquina: nem no ambiente, nem no arquivo.
    #[error("no key for the filter service")]
    MissingKey,
    /// O arquivo da chave vale, mas outros usuários da máquina podem lê-lo.
    /// Não impede o filtro: é o aviso para fechar a permissão.
    #[error("the key file {path} is readable by other users (mode {mode:o}): run chmod 600 {path}")]
    KeyFileOpen { path: String, mode: u32 },
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
            Self::KeyFileOpen { .. } => "key_file_open",
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
    /// Dá nota a cada candidato de `request` contra a frase e devolve o que
    /// passa do [`cut`], na ordem da nota, com o uso da chamada.
    fn filter(&self, request: &FilterRequest) -> Result<Filtered, FilterError>;
}

// ---------------------------------------------------------------------------
// O corte
// ---------------------------------------------------------------------------

// Os três números do corte medido com as perguntas reais dos agentes
// (`dt:0.4:12:0.15`): fica o que está a até 0,4 da melhor nota, nunca abaixo
// de 0,15, e no máximo 12 itens.

/// Quantos itens voltam, no máximo.
pub const MAX_KEPT: usize = 12;

/// O mínimo do corte que a busca pede, quando o projeto não diz outro. Com
/// 8, o corte empatou com 0 e com 6 nas perguntas escritas sobre o código,
/// achou 2 buscas a mais nas perguntas tiradas das mensagens do usuário e foi
/// o único que trouxe o serviço certo na simulação de uma tarefa real; custa
/// perto de 1 item a mais por busca.
pub const CUT_MINIMUM: usize = 8;

/// A nota abaixo da qual nada volta, nem para completar o mínimo.
pub const SCORE_FLOOR: f64 = 0.15;

/// A distância da melhor nota até a última que ainda volta.
pub const SCORE_SPREAD: f64 = 0.4;

/// O corte: ordena pela nota, da maior para a menor, e no empate fica a ordem
/// de `scores` (a do banco). Ficam as notas de pelo menos
/// `max(SCORE_FLOOR, melhor − SCORE_SPREAD)`, até [`MAX_KEPT`].
///
/// Com `minimum` maior que zero, se passarem menos que ele, voltam os
/// `minimum` primeiros pela nota, só entre os de nota de pelo menos
/// [`SCORE_FLOOR`].
#[must_use]
pub fn cut(scores: &[Scored], minimum: usize) -> Vec<Scored> {
    let mut ranked = scores.to_vec();
    // `sort_by` é estável: no empate, a ordem do banco fica.
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
    let Some(best) = ranked.first().map(|s| s.score) else {
        return Vec::new();
    };
    let threshold = SCORE_FLOOR.max(best - SCORE_SPREAD);
    let kept: Vec<Scored> = ranked.iter().filter(|s| s.score >= threshold).take(MAX_KEPT).copied().collect();
    if kept.len() >= minimum {
        return kept;
    }
    ranked.into_iter().filter(|s| s.score >= SCORE_FLOOR).take(minimum).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As notas na ordem do banco, com os ids 1, 2, 3…
    fn scores(notes: &[f64]) -> Vec<Scored> {
        notes.iter().enumerate().map(|(i, &score)| Scored { id: i as i64 + 1, score }).collect()
    }

    fn ids(kept: &[Scored]) -> Vec<i64> {
        kept.iter().map(|s| s.id).collect()
    }

    #[test]
    fn the_cut_keeps_what_is_close_to_the_best_score() {
        // 0,9 − 0,4 = 0,5: ficam 0,9 e 0,6; 0,45 e 0,1 caem.
        let kept = cut(&scores(&[0.9, 0.6, 0.45, 0.1]), 0);
        assert_eq!(ids(&kept), vec![1, 2]);
        assert!((kept[0].score - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn the_cut_line_sits_at_the_spread_below_the_best() {
        // A divisa: a nota igual a melhor − 0,4 volta; um fio abaixo, não.
        let edge = 0.9 - SCORE_SPREAD;
        let kept = cut(&scores(&[0.9, edge, edge - 1e-9]), 0);
        assert_eq!(ids(&kept), vec![1, 2]);
    }

    #[test]
    fn the_cut_never_goes_below_the_floor() {
        // Com a melhor em 0,3, a linha seria −0,1; o piso de 0,15 a segura.
        let kept = cut(&scores(&[0.3, SCORE_FLOOR, SCORE_FLOOR - 1e-9]), 0);
        assert_eq!(ids(&kept), vec![1, 2]);
    }

    #[test]
    fn the_cut_returns_at_most_twelve() {
        let kept = cut(&scores(&[0.8; 15]), 0);
        assert_eq!(ids(&kept), (1..=12).collect::<Vec<_>>());
    }

    #[test]
    fn the_bank_order_breaks_ties() {
        let kept = cut(&scores(&[0.5, 0.7, 0.5, 0.7]), 0);
        assert_eq!(ids(&kept), vec![2, 4, 1, 3]);
    }

    #[test]
    fn the_minimum_with_scores_that_already_pass_keeps_the_first_two() {
        let kept = cut(&scores(&[0.3, 0.2, 0.1]), 2);
        assert_eq!(ids(&kept), vec![1, 2]);
    }

    #[test]
    fn the_minimum_brings_back_the_next_best_above_the_floor() {
        // Só 0,95 passa da linha (0,55); com mínimo 3, voltam 0,95, 0,5 e 0,2,
        // e o 0,1, abaixo do piso, fica fora mesmo faltando item.
        let bank = scores(&[0.1, 0.5, 0.95, 0.2]);
        assert_eq!(ids(&cut(&bank, 0)), vec![3]);
        assert_eq!(ids(&cut(&bank, 3)), vec![3, 2, 4]);
        assert_eq!(ids(&cut(&bank, 5)), vec![3, 2, 4]);
    }

    #[test]
    fn nothing_to_cut_returns_nothing() {
        assert!(cut(&[], 3).is_empty());
    }
}
