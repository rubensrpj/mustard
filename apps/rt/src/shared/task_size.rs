//! O tamanho que a montagem espera de cada tarefa do backlog, em tokens de
//! conversa do agente.
//!
//! O Jev dá a cada tarefa uma nota de 0 a 3 na pergunta de tamanho: quanto
//! código o agente lê e muda para fazê-la. A tabela deste módulo traduz a nota
//! em quanto a conversa do agente cresce, e a montagem
//! ([`crate::shared::dag::pack_by_kind`]) fecha o lote do tipo quando a soma
//! das tarefas chega ao horizonte de trabalho adicional da onda. Entrada
//! inicial e resumos herdados precedem esse horizonte.
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
/// tokens, do menor para o maior nível. A nota não interpola entre dois
/// níveis: a documentação do Jev pede para não fazer conta com a nota.
pub(crate) const GROWTH_TOKENS: [u64; 4] = [35_000, 65_000, 95_000, 125_000];

/// Use the same conservative local estimate in packing and final cleanups.
pub(crate) fn native_growth(files: usize) -> u64 {
    growth_tokens(match files { 1 => 0.0, 2..=3 => 1.0, 4..=6 => 2.0, _ => 3.0 })
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

/// O nível de tamanho que a resposta `answer` dá, de 0 a 3: o de maior
/// probabilidade em `probabilities`, e o maior deles no empate. Sem
/// probabilidades legíveis, o nível mais próximo da nota; `None` quando a
/// resposta não traz nota.
pub(crate) fn level_in(answer: &Value) -> Option<f64> {
    let top = (LEVELS.len() - 1) as f64;
    let score = answer.get("score").and_then(Value::as_f64)?;
    let likeliest = answer.get("probabilities").and_then(Value::as_object).and_then(|chances| {
        (0..LEVELS.len())
            .filter_map(|level| chances.get(&level.to_string()).and_then(Value::as_f64).map(|chance| (level, chance)))
            .reduce(|best, next| if next.1 >= best.1 { next } else { best })
    });
    Some(likeliest.map_or_else(|| score.clamp(0.0, top).round(), |(level, _)| level as f64))
}

/// Quantos tokens a conversa do agente cresce ao fazer uma tarefa de nível
/// `level`: o valor da tabela no nível mais próximo, sem reta entre dois
/// níveis. Nível fora de 0 a 3 vale o mais próximo, e o que não é número, o
/// maior.
pub(crate) fn growth_tokens(level: f64) -> u64 {
    let top = GROWTH_TOKENS.len() - 1;
    let level = if level.is_nan() { top as f64 } else { level.clamp(0.0, top as f64) };
    GROWTH_TOKENS[level.round() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cada nível vale o que a tabela diz. A nota 1,4 com a maior
    /// probabilidade no nível 1 vale o nível 1, sem reta entre dois níveis; o
    /// empate vai ao maior nível.
    #[test]
    fn a_task_is_worth_the_table_value_of_its_likeliest_level() {
        let at: Vec<u64> = [0.0, 1.0, 2.0, 3.0].into_iter().map(growth_tokens).collect();
        assert_eq!(at, vec![35_000, 65_000, 95_000, 125_000]);
        let likeliest = |answer: Value| growth_tokens(level_in(&answer).unwrap());
        assert_eq!(likeliest(json!({"score": 1.4, "probabilities": {"0": 0.2, "1": 0.45, "2": 0.3, "3": 0.05}})), 65_000);
        assert_eq!(likeliest(json!({"score": 1.5, "probabilities": {"0": 0.0, "1": 0.5, "2": 0.5, "3": 0.0}})), 95_000);
        assert_eq!(likeliest(json!({"score": 2.2})), 95_000);
    }

    /// A nota fora dos quatro níveis vale o nível mais próximo, e a que não é
    /// número, o maior.
    #[test]
    fn a_score_outside_the_levels_is_worth_the_nearest_level() {
        assert_eq!(growth_tokens(-1.0), 35_000);
        assert_eq!(growth_tokens(7.0), 125_000);
        assert_eq!(growth_tokens(f64::NAN), 125_000);
    }

    /// A pergunta leva a tarefa pela chave, a nota de quatro níveis em ordem,
    /// e a resposta sem probabilidades é lida do nível mais próximo da nota,
    /// preso ao intervalo de 0 a 3.
    #[test]
    fn the_question_names_the_task_and_has_four_levels_and_the_answer_is_read_from_the_score() {
        let asked = question("t11");
        assert_eq!(asked["type"], json!("score"));
        assert!(asked["instructions"]["question"].as_str().unwrap().contains("`backlog.t11`"));
        assert_eq!(asked["criteria"].as_array().unwrap().len(), 4);
        assert_eq!(level_in(&json!({"type": "score", "score": 2.4, "confidence": 0.7})), Some(2.0));
        assert_eq!(level_in(&json!({"score": 9.0})), Some(3.0));
        assert_eq!(level_in(&json!({"type": "noul", "noul": 0.5})), None);
        assert_eq!(level_in(&json!({"score": "big"})), None);
    }
}
