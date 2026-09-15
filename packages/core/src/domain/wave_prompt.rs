//! O pedido de uma onda: o texto que o agente dela recebe, montado dos blocos
//! já lidos do arquivo de eventos.
//!
//! Tudo aqui é puro: sem disco, sem relógio e sem caminho da máquina. Quem lê
//! o arquivo, o banco de lições e os arquivos das skills entrega os blocos
//! prontos em [`Material`]; esta função só os escreve, sempre na mesma ordem,
//! então o mesmo material dá sempre os mesmos bytes.
//!
//! O pedido traz tudo escrito dentro dele, nunca "vá ler o arquivo X", e é
//! medido em linhas: acima de [`MAX_LINES`] ele é recusado, e quem monta o
//! plano divide a onda. Os eventos entram na versão vigente e sem o campo de
//! busca: o que aparece é o texto original de cada item.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::domain::spec_events::{type_spec, Kind, Refusal, SpecEvent};
use crate::platform::i18n::{translate, Locale};

/// O teto de linhas de um pedido de onda.
pub const MAX_LINES: usize = 500;

/// O texto de uma skill que uma tarefa da onda nomeia, copiado no pedido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// O nome pelo qual a tarefa a chama.
    pub name: String,
    /// O texto inteiro do arquivo da skill.
    pub text: String,
    /// `true` quando um dos exemplos que a skill usa mudou depois dela: o
    /// pedido a marca como a revisar.
    pub stale: bool,
}

/// Os blocos já lidos de que o pedido de uma onda é feito.
#[derive(Debug, Default)]
pub struct Material<'a> {
    /// O nome da spec.
    pub spec: String,
    /// O número da onda.
    pub wave: u64,
    /// O bloco da onda: a onda, as tarefas dela e as skills que elas nomeiam.
    pub block: Vec<&'a SpecEvent>,
    /// Os critérios que a onda aponta.
    pub criteria: Vec<&'a SpecEvent>,
    /// A especificação: contexto e preocupações.
    pub specification: Vec<&'a SpecEvent>,
    /// Os itens combinados escolhidos para esta onda.
    pub agreed: Vec<&'a SpecEvent>,
    /// O entregou das ondas de que esta depende.
    pub delivered: Vec<&'a SpecEvent>,
    /// As lições que valem para os arquivos, o subprojeto ou a skill da onda.
    pub lessons: Vec<&'a SpecEvent>,
    /// O texto das skills nomeadas pelas tarefas, na ordem dos nomes.
    pub skills: Vec<Skill>,
    /// O código de cada evento, para o pedido citar item por código.
    pub codes: BTreeMap<u64, String>,
}

/// O pedido montado e o tamanho dele.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// O texto inteiro do pedido.
    pub text: String,
    /// Quantas linhas ele tem.
    pub lines: usize,
}

/// Monta o pedido da onda a partir do material já lido.
///
/// # Errors
///
/// [`Refusal::WavePromptTooLong`] quando o pedido passa de [`MAX_LINES`]
/// linhas: a onda precisa ser dividida antes de ser despachada.
pub fn build(material: &Material, lang: Locale) -> Result<Prompt, Refusal> {
    let text = write(material, lang);
    let lines = count_lines(&text);
    if lines > MAX_LINES {
        return Err(Refusal::WavePromptTooLong { wave: material.wave, lines, max: MAX_LINES });
    }
    Ok(Prompt { text, lines })
}

/// O texto do pedido, sem medir nem recusar: a página mostra mesmo o pedido
/// grande demais, que é justamente o que precisa ser visto antes da aprovação.
#[must_use]
pub fn write(material: &Material, lang: Locale) -> String {
    let w = Writer { material, lang };
    w.text()
}

/// Quantas linhas um texto tem; a última conta mesmo sem quebra no fim.
#[must_use]
pub fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.lines().count()
}

struct Writer<'a> {
    material: &'a Material<'a>,
    lang: Locale,
}

impl Writer<'_> {
    fn t(&self, key: &str) -> &'static str {
        translate(key, self.lang)
    }

    fn text(&self) -> String {
        let m = self.material;
        let mut out = String::new();
        out.push_str(&format!(
            "# {}\n\n",
            self.t("prompt.title").replace("{spec}", &m.spec).replace("{n}", &m.wave.to_string())
        ));
        out.push_str(self.t("prompt.fixed"));
        out.push_str("\n\n");
        self.part(&mut out, "prompt.part.specification", &m.specification);
        self.part(&mut out, "prompt.part.agreed", &m.agreed);
        self.part(&mut out, "prompt.part.wave", &m.block);
        self.part(&mut out, "prompt.part.criteria", &m.criteria);
        self.lessons(&mut out);
        self.skills(&mut out);
        self.part(&mut out, "prompt.part.delivered", &m.delivered);
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }

    /// Uma parte do pedido: o título e um item por evento, na ordem em que o
    /// bloco foi lido. A parte sem nenhum evento não aparece.
    fn part(&self, out: &mut String, key: &str, events: &[&SpecEvent]) {
        if events.is_empty() {
            return;
        }
        out.push_str(&format!("## {}\n\n", self.t(key)));
        for event in events {
            self.event(out, event);
        }
    }

    /// Um evento: o código e o tipo no subtítulo, o texto original inteiro e
    /// os outros campos, um por linha.
    fn event(&self, out: &mut String, event: &SpecEvent) {
        let code = self.material.codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());
        let kind = self.t(&format!("page.type.{}", event.event_type));
        out.push_str(&format!("### {code} ({kind})\n\n"));
        if let Some(text) = event.str_field("text").map(str::trim).filter(|t| !t.is_empty()) {
            out.push_str(text);
            out.push_str("\n\n");
        }
        for line in self.fields(event) {
            out.push_str(&line);
            out.push('\n');
        }
        out.push('\n');
    }

    /// Os campos do evento fora do texto, na ordem em que o tipo os declara.
    /// O campo de busca nunca sai: o pedido mostra só o texto original.
    fn fields(&self, event: &SpecEvent) -> Vec<String> {
        let Some(spec) = type_spec(&event.event_type) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for field in spec.fields {
            if field.name == "text" || field.name == "search" {
                continue;
            }
            let Some(value) = event.fields.get(field.name) else {
                continue;
            };
            let shown = self.value(field.name, field.kind, value);
            if shown.is_empty() {
                continue;
            }
            let label = self.t(&format!("page.field.{}", field.name));
            out.push(format!("- {label}: {shown}"));
        }
        out
    }

    /// Um valor em uma linha. Os campos que apontam eventos saem pelo código
    /// do item, que é como o pedido, a página e a conversa o chamam.
    fn value(&self, name: &str, kind: Kind, value: &Value) -> String {
        let code = |id: u64| self.material.codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
        if matches!(name, "closes" | "criterion" | "reply_to")
            && let Some(id) = value.as_u64()
        {
            return code(id);
        }
        if matches!(name, "criteria" | "covers" | "items" | "targets" | "contracts")
            && matches!(kind, Kind::Ints | Kind::Refs)
        {
            return join(value.as_array().into_iter().flatten().map(|v| v.as_u64().map_or_else(|| flat(v), code)));
        }
        if name == "files" {
            return join(value.as_array().into_iter().flatten().map(|file| {
                let path = file.as_str().or_else(|| file.get("path").and_then(Value::as_str)).unwrap_or_default();
                if file.get("new").and_then(Value::as_bool) == Some(true) {
                    format!("{path} ({})", self.t("page.value.new"))
                } else {
                    path.to_string()
                }
            }));
        }
        flat(value)
    }

    /// As lições: só o texto original de cada uma, nunca o campo de busca.
    fn lessons(&self, out: &mut String) {
        if self.material.lessons.is_empty() {
            return;
        }
        out.push_str(&format!("## {}\n\n", self.t("prompt.part.lessons")));
        for lesson in &self.material.lessons {
            let text = lesson.str_field("text").unwrap_or_default().trim();
            out.push_str(&format!("- {text}\n"));
        }
        out.push('\n');
    }

    /// O texto inteiro de cada skill que uma tarefa nomeia, copiado aqui
    /// porque o agente não herda as skills do projeto.
    fn skills(&self, out: &mut String) {
        if self.material.skills.is_empty() {
            return;
        }
        out.push_str(&format!("## {}\n\n", self.t("prompt.part.skills")));
        for skill in &self.material.skills {
            out.push_str(&format!("### {}", skill.name));
            if skill.stale {
                out.push_str(&format!(" ({})", self.t("prompt.skill.stale")));
            }
            out.push_str("\n\n");
            out.push_str(skill.text.trim_end());
            out.push_str("\n\n");
        }
    }
}

/// Qualquer valor em uma linha: lista separada por vírgula, objeto como pares.
fn flat(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.split_whitespace().collect::<Vec<_>>().join(" "),
        Value::Array(items) => join(items.iter().map(flat)),
        Value::Object(map) => {
            map.iter().map(|(k, v)| format!("{k}: {}", flat(v))).collect::<Vec<_>>().join("; ")
        }
    }
}

fn join(items: impl Iterator<Item = String>) -> String {
    items.filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{parse_log, render_line, stamp, BlockQuery, SpecLog, Step};
    use serde_json::json;

    /// Um arquivo de eventos escrito à mão, uma linha por evento.
    fn log(events: &[(&str, Value)]) -> SpecLog {
        let mut content = String::new();
        for (i, (event_type, body)) in events.iter().enumerate() {
            let id = i as u64 + 1;
            let mut map = crate::domain::spec_events::normalize(
                body.as_object().cloned().unwrap_or_default(),
                event_type,
            );
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, id, None, "2026-09-15T10:00:00-03:00")));
            content.push('\n');
        }
        parse_log(&content)
    }

    fn material<'a>(log: &'a SpecLog, wave: u64) -> Material<'a> {
        let block = log.block(BlockQuery::Wave(wave));
        let criteria: Vec<&SpecEvent> = log
            .step(&Step::Dispatch { wave })
            .into_iter()
            .filter(|e| e.event_type == "criterion")
            .collect();
        Material {
            spec: "teste".into(),
            wave,
            block,
            criteria,
            codes: log.codes(),
            ..Material::default()
        }
    }

    #[test]
    fn a_request_carries_the_wave_its_tasks_and_its_criteria() {
        let log = log(&[
            (
                "criterion",
                json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test"}),
            ),
            (
                "wave",
                json!({"n": 1, "text": "Primeira onda", "criteria": [1], "done_when": "a suíte passa"}),
            ),
            ("task", json!({"wave": 1, "text": "Escrever o motor", "files": [{"path": "src/a.rs"}]})),
        ]);
        let prompt = build(&material(&log, 1), Locale::PtBr).expect("cabe nas 500 linhas");
        assert!(prompt.text.contains("Primeira onda"), "{}", prompt.text);
        assert!(prompt.text.contains("Escrever o motor"), "{}", prompt.text);
        assert!(prompt.text.contains("a suíte passa"), "{}", prompt.text);
        assert!(prompt.text.contains("src/a.rs"), "{}", prompt.text);
        assert!(prompt.lines > 0 && prompt.lines == prompt.text.lines().count());
    }

    /// O mesmo material escrito duas vezes dá os mesmos bytes.
    #[test]
    fn the_same_material_always_gives_the_same_bytes() {
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        let first = build(&material(&log, 1), Locale::PtBr).unwrap();
        let again = build(&material(&log, 1), Locale::PtBr).unwrap();
        assert_eq!(first, again);
    }

    /// Um pedido acima do teto de linhas é recusado, e a mensagem diz quantas
    /// linhas ele tem e qual é o teto.
    #[test]
    fn a_request_over_the_line_limit_is_refused_saying_how_far_it_went() {
        let long = "uma linha do texto\n".repeat(MAX_LINES + 10);
        let log = log(&[(
            "wave",
            json!({"n": 1, "text": long, "criteria": [], "done_when": "pronto"}),
        )]);
        let refused = build(&material(&log, 1), Locale::PtBr).unwrap_err();
        assert_eq!(refused.reason(), "wave-prompt-too-long");
        let message = refused.message(Locale::PtBr);
        assert!(message.contains(&MAX_LINES.to_string()), "{message}");
        let written = count_lines(&write(&material(&log, 1), Locale::PtBr));
        assert!(written > MAX_LINES);
        assert!(message.contains(&written.to_string()), "{message}");
        assert!(!refused.message(Locale::EnUs).is_empty());
        // A página ainda mostra o pedido grande: é ele que precisa ser visto.
        assert!(write(&material(&log, 1), Locale::PtBr).contains("uma linha do texto"));
    }

    /// O texto de cada skill nomeada é copiado inteiro no pedido, e a skill
    /// cujo exemplo mudou depois dela sai marcada como a revisar.
    #[test]
    fn the_named_skills_are_copied_whole_and_a_stale_one_is_marked() {
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        let mut m = material(&log, 1);
        m.skills = vec![
            Skill { name: "add-run-command".into(), text: "# Passos\n\n1. A regra.\n".into(), stale: false },
            Skill { name: "add-hook-rule".into(), text: "# Regra nova\n".into(), stale: true },
        ];
        let prompt = build(&m, Locale::PtBr).unwrap();
        assert!(prompt.text.contains("1. A regra."), "{}", prompt.text);
        assert!(prompt.text.contains("# Regra nova"), "{}", prompt.text);
        let stale = translate("prompt.skill.stale", Locale::PtBr);
        assert!(prompt.text.contains(&format!("add-hook-rule ({stale})")), "{}", prompt.text);
        assert!(!prompt.text.contains(&format!("add-run-command ({stale})")), "{}", prompt.text);
    }

    /// A lição entra pelo texto original; o campo de busca nunca aparece.
    #[test]
    fn a_lesson_shows_its_original_text_and_never_the_search_field() {
        let bank = log(&[(
            "lesson",
            json!({"text": "Apagar a pasta quebra o cache", "keys": ["apagar"], "class": "defect"}),
        )]);
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        let mut m = material(&log, 1);
        m.lessons = bank.visible();
        let prompt = build(&m, Locale::PtBr).unwrap();
        assert!(prompt.text.contains("Apagar a pasta quebra o cache"), "{}", prompt.text);
        assert!(!prompt.text.contains("apag "), "{}", prompt.text);
        let search = bank.visible()[0].str_field("search").unwrap_or_default().to_string();
        assert!(!search.is_empty(), "a linha da lição guarda o campo de busca");
        assert!(!prompt.text.contains(&search), "{}", prompt.text);
    }

    /// As instruções fixas abrem todo pedido, no idioma do projeto.
    #[test]
    fn every_request_opens_with_the_same_fixed_instructions() {
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        for lang in [Locale::PtBr, Locale::EnUs] {
            let prompt = build(&material(&log, 1), lang).unwrap();
            assert!(prompt.text.contains(translate("prompt.fixed", lang)), "{lang:?}");
        }
    }
}
