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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde_json::Value;

use crate::domain::lessons::{applies_to, Scope};
use crate::domain::search;
use crate::domain::spec_events::{type_spec, Block, BlockQuery, Kind, Refusal, SpecEvent, SpecLog};
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

// ---------------------------------------------------------------------------
// O recorte dos itens combinados por onda
// ---------------------------------------------------------------------------

/// Os itens combinados que o binário escolhe para a onda `wave`.
///
/// Escolhe sozinho, por três caminhos que se somam: o "onde vale" do item
/// contra os arquivos e as skills das tarefas da onda; a mesma busca das
/// lições entre o campo de busca do item e o texto da onda e das tarefas; e o
/// `covers` da tarefa, que corrige a escolha quando quem escreve o plano quer
/// um item numa onda em que a busca não o colocaria.
///
/// Na dúvida, o item vai: o que não casa com nenhuma onda em especial vai
/// para todas, porque em geral são as regras gerais, e deixar de ligar custa
/// mais caro do que ligar.
#[must_use]
pub fn agreed_for(log: &SpecLog, wave: u64) -> Vec<&SpecEvent> {
    let items = agreed_items(log);
    let mut where_it_goes: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for n in wave_numbers(log) {
        for id in matched_by(log, n, &items) {
            where_it_goes.entry(id).or_default().insert(n);
        }
    }
    items
        .into_iter()
        .filter(|item| where_it_goes.get(&item.id).is_none_or(|waves| waves.contains(&wave)))
        .collect()
}

/// Os itens combinados que o recorte considera: os do bloco do combinado que
/// têm texto. O tipo de trabalho e os pontos do levantamento não têm, e não
/// são itens a implementar.
fn agreed_items(log: &SpecLog) -> Vec<&SpecEvent> {
    log.block(BlockQuery::Block(Block::Agreed))
        .into_iter()
        .filter(|e| e.str_field("text").is_some_and(|t| !t.trim().is_empty()))
        .collect()
}

/// Os números das ondas do plano, em ordem.
fn wave_numbers(log: &SpecLog) -> Vec<u64> {
    let mut numbers: BTreeSet<u64> = BTreeSet::new();
    for event in log.block(BlockQuery::Block(Block::Waves)) {
        if event.event_type == "wave" && let Some(n) = event.wave() {
            numbers.insert(n);
        }
    }
    numbers.into_iter().collect()
}

/// Os itens que casam com a onda `n`.
fn matched_by(log: &SpecLog, n: u64, items: &[&SpecEvent]) -> BTreeSet<u64> {
    let block = log.block(BlockQuery::Wave(n));
    let tasks: Vec<&SpecEvent> = block.iter().copied().filter(|e| e.event_type == "task").collect();
    let mut out: BTreeSet<u64> = BTreeSet::new();

    for id in tasks.iter().flat_map(|task| task.ints("covers")) {
        if let Some(item) = log.current(id) {
            out.insert(item.id);
        }
    }

    let files: Vec<String> = tasks.iter().flat_map(|task| task_files(task)).collect();
    let mut scopes = vec![Scope { files: files.clone(), subproject: None, skill: None }];
    for skill in tasks.iter().filter_map(|task| task.str_field("skill")) {
        scopes.push(Scope { files: files.clone(), subproject: None, skill: Some(skill.to_string()) });
    }
    for item in items {
        if scopes.iter().any(|scope| applies_to(item, scope)) {
            out.insert(item.id);
        }
    }

    let mut query = String::new();
    for event in &block {
        if matches!(event.event_type.as_str(), "wave" | "task")
            && let Some(text) = event.str_field("text")
        {
            query.push_str(text);
            query.push(' ');
        }
    }
    let docs = items.iter().map(|item| (item.id, item.str_field("search").unwrap_or_default()));
    for hit in search::search(docs, &query) {
        out.insert(hit.id);
    }
    out
}

/// Os caminhos que uma tarefa declara.
fn task_files(task: &SpecEvent) -> Vec<String> {
    task.fields
        .get("files")
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .filter_map(|file| file.as_str().or_else(|| file.get("path").and_then(Value::as_str)))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
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
        let _ = writeln!(
            out,
            "# {}\n",
            self.t("prompt.title").replace("{spec}", &m.spec).replace("{n}", &m.wave.to_string())
        );
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
        let _ = writeln!(out, "## {}\n", self.t(key));
        for event in events {
            self.event(out, event);
        }
    }

    /// Um evento: o código e o tipo no subtítulo, o texto original inteiro e
    /// os outros campos, um por linha.
    fn event(&self, out: &mut String, event: &SpecEvent) {
        let code = self.material.codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());
        let kind = self.t(&format!("page.type.{}", event.event_type));
        let _ = writeln!(out, "### {code} ({kind})\n");
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
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.lessons"));
        for lesson in &self.material.lessons {
            let text = lesson.str_field("text").unwrap_or_default().trim();
            let _ = writeln!(out, "- {text}");
        }
        out.push('\n');
    }

    /// O texto inteiro de cada skill que uma tarefa nomeia, copiado aqui
    /// porque o agente não herda as skills do projeto.
    fn skills(&self, out: &mut String) {
        if self.material.skills.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.skills"));
        for skill in &self.material.skills {
            let _ = write!(out, "### {}", skill.name);
            if skill.stale {
                let _ = write!(out, " ({})", self.t("prompt.skill.stale"));
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

    fn material(log: &SpecLog, wave: u64) -> Material<'_> {
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

    /// Um plano com duas ondas e seis regras, para provar o recorte.
    fn plan() -> SpecLog {
        log(&[
            (
                "rule",
                json!({"text": "No máximo 3 tentativas de compilação por onda", "keys": ["tentativas"],
                       "example": "a quarta tentativa para", "applies_to": {"files": ["src/b.rs"]}}),
            ),
            ("rule", json!({"text": "O commit segue o modelo aprovado", "keys": ["commit"], "example": "título curto"})),
            ("rule", json!({"text": "A barra de status mostra o link", "keys": ["barra"], "example": "duas linhas"})),
            (
                "rule",
                json!({"text": "O leitor do arquivo de eventos nunca lê o arquivo inteiro",
                       "keys": ["leitor"], "example": "um bloco por vez"}),
            ),
            ("rule", json!({"text": "A página do relatório sai do mesmo motor", "keys": ["página"], "example": "um motor só"})),
            ("wave", json!({"n": 1, "text": "Leitura", "criteria": [], "done_when": "lê"})),
            (
                "task",
                json!({"wave": 1, "text": "Escrever o leitor do arquivo de eventos",
                       "files": [{"path": "src/a.rs"}], "covers": [3]}),
            ),
            ("wave", json!({"n": 2, "text": "Página", "criteria": [], "done_when": "sai"})),
            ("task", json!({"wave": 2, "text": "Gravar a página do relatório", "files": [{"path": "src/b.rs"}]})),
        ])
    }

    fn texts(log: &SpecLog, wave: u64) -> Vec<String> {
        agreed_for(log, wave).iter().map(|e| e.str_field("text").unwrap_or_default().to_string()).collect()
    }

    /// Uma regra com limite numérico que vale para um arquivo da onda 2
    /// aparece no pedido dela, e não no da onda 1.
    #[test]
    fn a_rule_scoped_to_a_file_the_wave_touches_lands_in_that_waves_request() {
        let plan = plan();
        let rule = "No máximo 3 tentativas de compilação por onda".to_string();
        assert!(texts(&plan, 2).contains(&rule), "{:?}", texts(&plan, 2));
        assert!(!texts(&plan, 1).contains(&rule), "{:?}", texts(&plan, 1));
    }

    /// O item que não casa com nenhuma onda em especial vai para todas: na
    /// dúvida, o item vai.
    #[test]
    fn an_item_that_matches_no_wave_goes_to_every_wave() {
        let plan = plan();
        let general = "O commit segue o modelo aprovado".to_string();
        assert!(texts(&plan, 1).contains(&general), "{:?}", texts(&plan, 1));
        assert!(texts(&plan, 2).contains(&general), "{:?}", texts(&plan, 2));
    }

    /// O `covers` da tarefa corrige a escolha: o item que a busca não ligaria
    /// à onda entra nela, e continua fora das outras.
    #[test]
    fn the_tasks_covers_field_corrects_the_choice() {
        let plan = plan();
        let picked = "A barra de status mostra o link".to_string();
        assert!(texts(&plan, 1).contains(&picked), "{:?}", texts(&plan, 1));
        assert!(!texts(&plan, 2).contains(&picked), "{:?}", texts(&plan, 2));
    }

    /// A busca liga o item à onda cujo texto fala do mesmo assunto.
    #[test]
    fn the_search_links_an_item_to_the_wave_that_talks_about_it() {
        let plan = plan();
        assert!(texts(&plan, 1).contains(&"O leitor do arquivo de eventos nunca lê o arquivo inteiro".to_string()));
        assert!(texts(&plan, 2).contains(&"A página do relatório sai do mesmo motor".to_string()));
        assert!(!texts(&plan, 2).contains(&"O leitor do arquivo de eventos nunca lê o arquivo inteiro".to_string()));
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
