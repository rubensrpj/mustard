//! O tamanho que a montagem espera de cada tarefa do backlog, em tokens de
//! conversa do agente.
//!
//! O Jev dá a cada tarefa uma nota de 0 a 3 na pergunta de tamanho: quanto
//! código o agente lê e muda para fazê-la. A tabela deste módulo traduz a nota
//! em quanto a conversa do agente cresce, e a montagem
//! ([`crate::shared::dag::pack_by_kind`]) fecha o lote do tipo quando a soma
//! das tarefas chega ao teto: o limite do agente de onda menos o que a
//! conversa já pesa ao começar.
//!
//! ## De onde saiu a tabela
//!
//! Do teste de 03/10, que cruzou duas medidas das mesmas ondas: a nota do Jev
//! para o volume de trabalho, nos mesmos quatro níveis da pergunta abaixo
//! (`jev-orquestra/c_modelo.json`, campo `vol`), e o maior tamanho que a
//! conversa do agente chegou a ter (`prova-03-10/ondas.json`, campo `pico`),
//! menos os 40 mil do começo. Entraram só as 62 ondas de uma tarefa, de um
//! agente, sem compactação nem parada por resumo. As medianas por nível
//! arredondado ficaram em 26 mil (4 ondas), 66 mil (27), 80 mil (26) e 166 mil
//! (5); a reta pelas 62 ondas é 34 mil mais 30 mil por nível. A tabela usa a
//! reta, de 30 em 30 mil, em vez das medianas dos extremos, que repousam em 4
//! e 5 ondas. Onda de 2 a 3 tarefas cresceu menos que a soma das tarefas
//! sozinhas (de 86 a 140 mil), então somar é conservador: o lote erra para
//! menos tarefa, nunca para mais.

use serde_json::{json, Value};

/// Os quatro níveis da pergunta de tamanho, do menor para o maior, como o Jev
/// os lê. São os mesmos da medida que calibrou a tabela.
const LEVELS: [&str; 4] =
    ["One small place", "Two or three files", "Many files in one area", "Many files across several areas"];

/// Quanto a conversa do agente cresce ao fazer uma tarefa de cada nível, em
/// tokens, do menor para o maior nível. A nota entre dois níveis interpola.
pub(crate) const GROWTH_TOKENS: [u64; 4] = [35_000, 65_000, 95_000, 125_000];

/// Quanto a conversa do agente já pesa ao começar, antes da primeira tarefa:
/// o pedido, as regras do projeto e o que ele lê para se situar.
pub(crate) const AGENT_START_TOKENS: u64 = 40_000;

/// O teto da soma dos tamanhos das tarefas de uma onda, para um agente cuja
/// conversa pode chegar a `limit` tokens: o limite menos o que ela pesa ao
/// começar.
pub(crate) fn wave_budget(limit: u64) -> u64 {
    limit.saturating_sub(AGENT_START_TOKENS)
}

/// A pergunta de tamanho da tarefa de chave `key` (`t11`), no formato de nota
/// do serviço: o enunciado e os quatro níveis em ordem.
pub(crate) fn question(key: &str) -> Value {
    json!({
        "type": "score",
        "instructions": { "question": format!("How much code must the agent read and change to do `backlog.{key}`?") },
        "criteria": LEVELS,
    })
}

/// A nota de tamanho que a resposta `answer` dá, de 0 a 3; `None` quando a
/// resposta não traz nota. Nota fora dos quatro níveis vale o nível mais
/// próximo.
pub(crate) fn level_in(answer: &Value) -> Option<f64> {
    let top = (LEVELS.len() - 1) as f64;
    answer.get("score").and_then(Value::as_f64).map(|score| score.clamp(0.0, top))
}

/// Quantos tokens a conversa do agente cresce ao fazer uma tarefa de nota
/// `level`: o valor da tabela no nível, ou o da reta entre os dois níveis que
/// a nota separa. Nota fora de 0 a 3 vale o nível mais próximo.
pub(crate) fn growth_tokens(level: f64) -> u64 {
    let top = GROWTH_TOKENS.len() - 1;
    let level = if level.is_nan() { top as f64 } else { level.clamp(0.0, top as f64) };
    let below = level.floor() as usize;
    let above = level.ceil() as usize;
    let (from, to) = (GROWTH_TOKENS[below] as f64, GROWTH_TOKENS[above] as f64);
    (from + (to - from) * (level - below as f64)).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cada nível vale o que a tabela diz, e a nota entre dois níveis fica na
    /// reta entre eles.
    #[test]
    fn a_level_is_worth_its_table_value_and_a_score_between_two_levels_is_on_the_line() {
        let at: Vec<u64> = [0.0, 1.0, 2.0, 3.0].into_iter().map(growth_tokens).collect();
        assert_eq!(at, vec![35_000, 65_000, 95_000, 125_000]);
        assert_eq!(growth_tokens(0.5), 50_000);
        assert_eq!(growth_tokens(1.5), 80_000);
        assert_eq!(growth_tokens(2.2), 101_000);
    }

    /// A nota fora dos quatro níveis vale o nível mais próximo, e a que não é
    /// número, o maior.
    #[test]
    fn a_score_outside_the_levels_is_worth_the_nearest_level() {
        assert_eq!(growth_tokens(-1.0), 35_000);
        assert_eq!(growth_tokens(7.0), 125_000);
        assert_eq!(growth_tokens(f64::NAN), 125_000);
    }

    /// O teto de um agente que pode ir a 180 mil tokens é 140 mil: os 40 mil
    /// do começo da conversa ficam de fora, e nenhum limite menor que eles dá
    /// teto negativo.
    #[test]
    fn the_budget_is_the_limit_minus_what_the_conversation_weighs_at_the_start() {
        assert_eq!(wave_budget(180_000), 140_000);
        assert_eq!(wave_budget(30_000), 0);
        assert_eq!(
            wave_budget(crate::hooks::session::conversation_size::WAVE_LIMIT),
            140_000,
            "o limite do agente de onda mudou: a tabela precisa de nova conta"
        );
    }

    /// A pergunta leva a tarefa pela chave, a nota de quatro níveis em ordem,
    /// e a resposta dela é lida da nota e presa ao intervalo de 0 a 3.
    #[test]
    fn the_question_names_the_task_and_has_four_levels_and_the_answer_is_read_from_the_score() {
        let asked = question("t11");
        assert_eq!(asked["type"], json!("score"));
        assert!(asked["instructions"]["question"].as_str().unwrap().contains("`backlog.t11`"));
        assert_eq!(asked["criteria"].as_array().unwrap().len(), 4);
        assert_eq!(level_in(&json!({"type": "score", "score": 2.4, "confidence": 0.7})), Some(2.4));
        assert_eq!(level_in(&json!({"score": 9.0})), Some(3.0));
        assert_eq!(level_in(&json!({"type": "noul", "noul": 0.5})), None);
        assert_eq!(level_in(&json!({"score": "big"})), None);
    }
}
