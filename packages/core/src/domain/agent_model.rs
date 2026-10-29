//! O modelo do agente de cada onda, pela dificuldade das tarefas dela, e a
//! subida de modelo quando a onda é reprovada.
//!
//! Na formação das ondas, o Jev dá a cada tarefa uma nota de complexidade de
//! quatro níveis, com a confiança da leitura, e a formação a grava na tarefa
//! (`group.difficulty`). O modelo sai da nota: os níveis 0, 1 e 2 vão ao
//! Sonnet e o 3 ao Opus — o Haiku não escreve código —; a confiança abaixo de
//! [`LOW_CONFIDENCE`] sobe um degrau, porque a leitura incerta pede cautela.
//! A tarefa sem nota vai ao Sonnet ([`DEFAULT_AGENT_MODEL`]), nunca ao modelo
//! de `agents.model`, que só vale com a escolha pela dificuldade desligada; a
//! onda leva o maior modelo das tarefas dela. A onda reprovada volta um degrau acima do modelo
//! do envio anterior; o Opus fica no Opus.

use serde_json::{Value, json};

use crate::domain::config::DEFAULT_AGENT_MODEL;
use crate::domain::spec_events::{SpecEvent, SpecLog};

/// Os degraus de modelo, do mais leve ao mais pesado, pelos apelidos que a
/// ferramenta Agent do Claude Code aceita.
pub const LADDER: [&str; 3] = ["haiku", "sonnet", "opus"];

/// A confiança da nota abaixo da qual o modelo sobe um degrau: a
/// documentação do Jev trata a leitura abaixo dela como incerta.
pub const LOW_CONFIDENCE: f64 = 0.5;

/// Quantos níveis a nota de complexidade tem.
const LEVELS: u64 = 4;

/// A nota de complexidade que o Jev deu a uma tarefa na formação das ondas:
/// o nível, de 0 a 3, e a confiança da leitura, de 0 a 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Difficulty {
    pub level: u64,
    pub confidence: f64,
}

impl Difficulty {
    /// A nota gravada na tarefa `task`, em `group.difficulty`; `None` sem ela
    /// ou com o nível fora dos quatro.
    #[must_use]
    pub fn of(task: &SpecEvent) -> Option<Self> {
        let said = task.fields.get("group")?.get("difficulty")?;
        let level = said.get("level")?.as_u64().filter(|level| *level < LEVELS)?;
        let confidence = said.get("confidence")?.as_f64()?;
        Some(Self { level, confidence })
    }

    /// A nota como a formação a grava na tarefa.
    #[must_use]
    pub fn to_value(self) -> Value {
        json!({ "level": self.level, "confidence": self.confidence })
    }

    /// O modelo que a nota pede: os níveis 0, 1 e 2 vão ao Sonnet e o 3 ao
    /// Opus, um degrau acima com a confiança abaixo de [`LOW_CONFIDENCE`].
    /// Nenhuma nota manda escrever código no Haiku: a implementação simples é
    /// do Sonnet, por decisão do usuário.
    #[must_use]
    pub fn model(self) -> &'static str {
        let step = match self.level {
            0..=2 => 1,
            _ => 2,
        };
        let step = if self.confidence < LOW_CONFIDENCE { step + 1 } else { step };
        LADDER[step.min(LADDER.len() - 1)]
    }
}

/// O degrau de `model`, pelo apelido que ele contém (`claude-opus-5-5` é o
/// Opus); `None` no modelo fora dos degraus.
fn step_of(model: &str) -> Option<usize> {
    let lower = model.to_ascii_lowercase();
    LADDER.iter().position(|step| lower.contains(step))
}

/// O degrau acima de `model`; `None` no Opus, que fica, e no modelo fora dos
/// degraus, que ninguém sobe.
#[must_use]
pub fn step_up(model: &str) -> Option<&'static str> {
    step_of(model).and_then(|step| LADDER.get(step + 1).copied())
}

/// O maior dos dois modelos. O modelo fora dos degraus é o que a
/// configuração do projeto escolheu num envio anterior e vence os degraus.
#[must_use]
pub fn larger<'a>(one: &'a str, other: &'a str) -> &'a str {
    match (step_of(one), step_of(other)) {
        (Some(a), Some(b)) if b > a => other,
        (Some(_), None) => other,
        _ => one,
    }
}

/// A subida de modelo de um envio: o modelo do envio anterior (`from`) e o
/// evento que a causou (`cause`) — o veredito que reprovou a onda ou a volta
/// dela que a rodada recusou ou quem conduz reprovou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escalation {
    pub from: String,
    pub cause: u64,
}

/// O campo do envio que guarda a subida de modelo.
pub const ESCALATED_FIELD: &str = "escalated_from";

impl Escalation {
    /// A subida gravada no envio `send`, em [`ESCALATED_FIELD`].
    #[must_use]
    pub fn of(send: &SpecEvent) -> Option<Self> {
        let said = send.fields.get(ESCALATED_FIELD)?;
        Some(Self { from: said.get("model")?.as_str()?.to_string(), cause: said.get("cause")?.as_u64()? })
    }

    /// A subida como o envio a grava.
    #[must_use]
    pub fn to_value(&self) -> Value {
        json!({ "model": self.from, "cause": self.cause })
    }
}

/// O modelo de um envio de onda e, quando ele subiu por uma reprovação, de
/// onde subiu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveModel {
    pub model: String,
    pub escalated_from: Option<Escalation>,
}

/// O modelo do envio que sobe um degrau acima de `previous` pela reprovação
/// `cause`, sem ficar abaixo de `floor`. O Opus fica, e o modelo fora dos
/// degraus também: sem subida, nada se grava.
#[must_use]
pub fn escalate(previous: &str, floor: &str, cause: u64) -> WaveModel {
    let model = larger(floor, step_up(previous).unwrap_or(previous));
    let rose = matches!((step_of(previous), step_of(model)), (Some(before), Some(after)) if after > before);
    WaveModel { model: model.to_string(), escalated_from: rose.then(|| Escalation { from: previous.to_string(), cause }) }
}

/// O modelo da onda `wave` de `log`: o maior das tarefas dela, cada uma pela
/// nota dela, ou o Sonnet quando não tem nota. Reprovada por um veredito
/// depois do despacho do envio anterior, a onda sobe um degrau acima do
/// modelo dele.
#[must_use]
pub fn wave_model(log: &SpecLog, wave: u64) -> WaveModel {
    let floor = log
        .visible()
        .into_iter()
        .filter(|event| event.event_type == "task" && event.wave() == Some(wave))
        .map(|task| Difficulty::of(task).map_or(DEFAULT_AGENT_MODEL, Difficulty::model))
        .reduce(larger)
        .unwrap_or(DEFAULT_AGENT_MODEL);
    let previous = log.last_by_wave("send").get(&wave).and_then(|sent| log.send_field(*sent, "model")).and_then(Value::as_str);
    match (previous, rejected_since_dispatch(log, wave)) {
        (Some(previous), Some(cause)) => escalate(previous, floor, cause),
        _ => WaveModel { model: floor.to_string(), escalated_from: None },
    }
}

/// O veredito mais novo que reprovou a onda `wave` depois do despacho do
/// último envio dela.
fn rejected_since_dispatch(log: &SpecLog, wave: u64) -> Option<u64> {
    let dispatched = *log.last_dispatch_by_wave().get(&wave)?;
    let verdicts = log.verdicts_by_wave();
    verdicts
        .get(&wave)?
        .iter()
        .filter(|verdict| verdict.event_type == "verdict" && verdict.str_field("result") == Some("rejected") && verdict.id > dispatched)
        .map(|verdict| verdict.id)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{normalize, parse_log, render_line, stamp};

    fn log(events: &[(&str, Value)]) -> SpecLog {
        let mut content = String::new();
        for (at, (event_type, body)) in events.iter().enumerate() {
            let mut map = normalize(body.as_object().cloned().unwrap_or_default(), event_type);
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, at as u64 + 1, None, "2026-10-19T10:00:00-03:00")));
            content.push('\n');
        }
        parse_log(&content)
    }

    fn task(wave: u64, difficulty: Option<(u64, f64)>) -> (&'static str, Value) {
        let mut body = json!({ "wave": wave, "title": "Tarefa", "text": "Tarefa.", "files": [{ "path": "src/a.rs" }] });
        if let Some((level, confidence)) = difficulty {
            body["group"] = json!({ "name": "Assunto", "position": 1, "formed": true, "difficulty": { "level": level, "confidence": confidence } });
        }
        ("task", body)
    }

    /// The four levels go to Haiku, Sonnet, Sonnet and Opus, and a reading
    /// below the confidence floor goes one step up, Opus staying.
    #[test]
    fn each_level_goes_to_its_model_and_a_low_confidence_goes_one_up() {
        let model = |level, confidence| Difficulty { level, confidence }.model();
        assert_eq!([model(0, 0.9), model(1, 0.9), model(2, 0.9), model(3, 0.9)], ["sonnet", "sonnet", "sonnet", "opus"]);
        assert_eq!([model(0, 0.49), model(1, 0.2), model(2, 0.0), model(3, 0.1)], ["opus", "opus", "opus", "opus"]);
        assert_eq!(model(0, LOW_CONFIDENCE), "sonnet", "the floor itself is not low");
    }

    /// The wave takes the largest model of its tasks; a task without a grade
    /// counts as Sonnet, never as the configured model, so it does not go to
    /// Opus.
    #[test]
    fn the_wave_takes_the_largest_model_of_its_tasks() {
        let graded = log(&[task(1, Some((0, 0.9))), task(1, Some((3, 0.9))), task(2, Some((0, 0.9))), task(2, Some((0, 0.95)))]);
        assert_eq!(wave_model(&graded, 1).model, "opus");
        assert_eq!(wave_model(&graded, 2).model, "sonnet", "a simple wave is written on Sonnet, never on Haiku");
        let mixed = log(&[task(1, Some((0, 0.9))), task(1, None)]);
        assert_eq!(wave_model(&mixed, 1).model, "sonnet", "the task without a grade counts as Sonnet");
        let bare = log(&[task(1, None)]);
        assert_eq!(wave_model(&bare, 1), WaveModel { model: "sonnet".into(), escalated_from: None }, "a task without a grade does not go to Opus");
        assert_eq!(wave_model(&log(&[task(1, Some((2, 0.9))), task(1, None)]), 1).model, "sonnet");
        assert_eq!(wave_model(&log(&[task(1, Some((3, 0.9)))]), 1).model, "opus", "a graded task goes by its grade");
    }

    /// A wave rejected by a verdict after its last send goes one step above
    /// that send's model and records where it came from; Opus stays, and a
    /// verdict before the send does not count.
    #[test]
    fn a_rejected_wave_goes_one_step_above_its_last_send() {
        let send = |model: &str| ("send", json!({ "wave": 1, "role": "wave", "text": "pedido", "lines": 1, "chars": 6, "mustard": "0", "model": model }));
        let rejected = ("verdict", json!({ "wave": 1, "result": "rejected", "text": "faltou o teste", "author": "review" }));
        let wave = ("wave", json!({ "n": 1, "text": "Onda.", "criteria": [], "done_when": "a suíte passa" }));
        let after = log(&[wave.clone(), task(1, Some((0, 0.9))), send("haiku"), rejected.clone()]);
        assert_eq!(wave_model(&after, 1), WaveModel { model: "sonnet".into(), escalated_from: Some(Escalation { from: "haiku".into(), cause: 4 }) });
        let twice = log(&[wave.clone(), task(1, Some((0, 0.9))), send("sonnet"), rejected.clone()]);
        assert_eq!(wave_model(&twice, 1).model, "opus");
        let top = log(&[wave.clone(), task(1, Some((0, 0.9))), send("opus"), rejected.clone()]);
        assert_eq!(wave_model(&top, 1), WaveModel { model: "opus".into(), escalated_from: None }, "opus stays");
        let before = log(&[wave, task(1, Some((0, 0.9))), rejected, send("sonnet")]);
        assert_eq!(wave_model(&before, 1).model, "sonnet", "a verdict before the send does not count");
        assert_eq!(escalate("claude-sonnet-5-5", "haiku", 7).model, "opus");
        assert_eq!(escalate("custom-model-1", "haiku", 7), WaveModel { model: "custom-model-1".into(), escalated_from: None });
    }
}
