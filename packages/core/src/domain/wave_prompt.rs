//! O pedido de uma onda: o texto que o agente dela recebe, montado dos blocos
//! já lidos do arquivo de eventos.
//!
//! Tudo aqui é puro: sem disco, sem relógio e sem caminho da máquina. Quem lê
//! o arquivo, o banco de lições e os arquivos das skills entrega os blocos
//! prontos em [`Material`]; esta função só os escreve, sempre na mesma ordem,
//! então o mesmo material dá sempre os mesmos bytes.
//!
//! O pedido cabe no teto de linhas por construção. Ele copia por inteiro a
//! especificação, a onda com as tarefas dela, os critérios, o item que vale
//! para todas as ondas e os itens que as tarefas declaram cobrir; todo o
//! resto do combinado entra como ponteiro de uma linha, com o código, o
//! título e o comando que lê o item inteiro. Ainda assim medido em linhas:
//! acima de [`MAX_LINES`] ele é recusado, e a recusa diz o que ficou inteiro
//! para quem for dividir a onda. Os eventos entram na versão vigente e sem o
//! campo de busca: o que aparece é o texto original de cada item.

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
/// linhas mesmo com o combinado reduzido a ponteiros: a onda precisa ser
/// dividida antes de ser despachada, e a recusa diz o que ficou inteiro.
pub fn build(material: &Material, lang: Locale) -> Result<Prompt, Refusal> {
    let text = write(material, lang);
    let lines = count_lines(&text);
    if lines > MAX_LINES {
        return Err(too_long(material, lines, lang));
    }
    Ok(Prompt { text, lines })
}

/// A recusa do teto de linhas, com as partes que ficaram inteiras e o tamanho
/// de cada uma: quem divide a onda precisa saber o que tirar dela.
#[must_use]
pub fn too_long(material: &Material, lines: usize, lang: Locale) -> Refusal {
    Refusal::WavePromptTooLong {
        wave: material.wave,
        lines,
        max: MAX_LINES,
        parts: parts(material, lang),
    }
}

/// O texto do pedido, sem medir nem recusar: a página mostra mesmo o pedido
/// grande demais, que é justamente o que precisa ser visto antes da aprovação.
#[must_use]
pub fn write(material: &Material, lang: Locale) -> String {
    write_with(material, &copied_whole(material, lang), lang)
}

/// O texto do pedido com os itens combinados de `whole` copiados por inteiro
/// e todos os outros como ponteiro.
fn write_with(material: &Material, whole: &BTreeSet<u64>, lang: Locale) -> String {
    Writer { material, whole, lang }.text()
}

/// As partes que o pedido copia por inteiro, cada uma com quantas linhas
/// ocupa, separadas por vírgula.
fn parts(material: &Material, lang: Locale) -> String {
    let whole = copied_whole(material, lang);
    let w = Writer { material, whole: &whole, lang };
    let mut out: Vec<String> = Vec::new();
    let mut named = |key: &str, lines: usize| {
        if lines > 0 {
            out.push(format!("{} ({lines})", translate(key, lang)));
        }
    };
    named("prompt.part.specification", w.part_lines("prompt.part.specification", &material.specification));
    named("prompt.part.agreed", w.part_lines("prompt.part.agreed", &w.whole_agreed()));
    named("prompt.part.wave", w.part_lines("prompt.part.wave", &material.block));
    named("prompt.part.criteria", w.part_lines("prompt.part.criteria", &material.criteria));
    named("prompt.part.lessons", w.lessons_lines());
    named("prompt.part.skills", w.skills_lines());
    named("prompt.part.delivered", w.part_lines("prompt.part.delivered", &material.delivered));
    out.join(", ")
}

// ---------------------------------------------------------------------------
// O que entra inteiro e o que entra como ponteiro
// ---------------------------------------------------------------------------

/// Os itens combinados que o pedido copia por inteiro: o item que vale para
/// todas as ondas e os itens que as tarefas da onda declaram cobrir. Todo o
/// resto entra como ponteiro de uma linha, que o agente lê pelo binário
/// quando precisar.
///
/// A escolha enche por ordem de relevância — a mesma busca que liga item e
/// onda — até o teto de linhas; o que não couber vira ponteiro também.
fn copied_whole(material: &Material, lang: Locale) -> BTreeSet<u64> {
    let mut chosen: BTreeSet<u64> = BTreeSet::new();
    let candidates = by_relevance(material);
    if candidates.is_empty() {
        return chosen;
    }
    for id in candidates {
        let mut with_it = chosen.clone();
        with_it.insert(id);
        if count_lines(&write_with(material, &with_it, lang)) <= MAX_LINES {
            chosen = with_it;
        }
    }
    chosen
}

/// Os itens que podem ser copiados por inteiro, do mais relevante para o
/// menos: primeiro os que a busca liga ao texto da onda e das tarefas, na
/// ordem da nota, depois os que ela não liga, na ordem em que foram gravados.
fn by_relevance(material: &Material) -> Vec<u64> {
    let covered: BTreeSet<u64> = material
        .block
        .iter()
        .filter(|event| event.event_type == "task")
        .flat_map(|task| task.ints("covers"))
        .collect();
    let candidates: Vec<&SpecEvent> = material
        .agreed
        .iter()
        .copied()
        .filter(|item| covered.contains(&item.id) || every_wave(item))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let mut query = String::new();
    for event in &material.block {
        if matches!(event.event_type.as_str(), "wave" | "task")
            && let Some(text) = event.str_field("text")
        {
            query.push_str(text);
            query.push(' ');
        }
    }
    let docs = candidates.iter().map(|item| (item.id, item.str_field("search").unwrap_or_default()));
    let index = search::SearchIndex::build(docs);
    let ranked = index.top(&search::query_terms(&query), candidates.len());
    let mut out: Vec<u64> = ranked.iter().map(|hit| hit.id).collect();
    let rest: Vec<u64> =
        candidates.iter().map(|item| item.id).filter(|id| !out.contains(id)).collect();
    out.extend(rest);
    out
}

/// O item marcado como válido para todas as ondas: o "onde vale" dele cobre
/// o projeto inteiro, e não um arquivo, um subprojeto ou uma skill.
fn every_wave(item: &SpecEvent) -> bool {
    applies_to(item, &Scope::default())
}

/// O ponteiro de um item: o código, o título e, na mesma linha, o comando que
/// lê o item inteiro pelo binário.
fn pointer(material: &Material, item: &SpecEvent) -> String {
    let code = material.codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string());
    let title = title_of(item);
    format!(
        "- {code} — {title} — `mustard-rt run read agreed --spec {} --term {code}`",
        material.spec
    )
}

/// Quantos caracteres do título de um item cabem no ponteiro.
const TITLE_CHARS: usize = 80;

/// O título de um item: a primeira linha do texto dele, sem o negrito que a
/// abre e cortada no fim de uma palavra quando é longa demais.
fn title_of(item: &SpecEvent) -> String {
    let text = item.str_field("text").unwrap_or_default();
    let first = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let first = first.strip_prefix("**").map_or(first, |rest| rest.split("**").next().unwrap_or(rest));
    let title: String = first.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.chars().count() <= TITLE_CHARS {
        return title;
    }
    let mut cut = String::new();
    for word in title.split(' ') {
        if cut.chars().count() + word.chars().count() + 1 > TITLE_CHARS {
            break;
        }
        if !cut.is_empty() {
            cut.push(' ');
        }
        cut.push_str(word);
    }
    format!("{}…", cut.trim_end_matches([' ', ',', ';', '.']))
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
    /// Os itens combinados copiados por inteiro; os outros viram ponteiro.
    whole: &'a BTreeSet<u64>,
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
        self.part(&mut out, "prompt.part.agreed", &self.whole_agreed());
        self.pointers(&mut out);
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

    /// Os itens combinados que entram por inteiro, na ordem em que o bloco
    /// foi lido.
    fn whole_agreed(&self) -> Vec<&SpecEvent> {
        self.material.agreed.iter().copied().filter(|item| self.whole.contains(&item.id)).collect()
    }

    /// Os itens combinados que entram como ponteiro: uma linha cada, com o
    /// código, o título e o comando que lê o item inteiro.
    fn pointers(&self, out: &mut String) {
        let rest: Vec<&SpecEvent> =
            self.material.agreed.iter().copied().filter(|item| !self.whole.contains(&item.id)).collect();
        if rest.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.pointers"));
        for item in rest {
            let _ = writeln!(out, "{}", pointer(self.material, item));
        }
        out.push('\n');
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

    /// Quantas linhas uma parte ocupa sozinha.
    fn part_lines(&self, key: &str, events: &[&SpecEvent]) -> usize {
        let mut out = String::new();
        self.part(&mut out, key, events);
        count_lines(&out)
    }

    /// Quantas linhas as lições ocupam.
    fn lessons_lines(&self) -> usize {
        let mut out = String::new();
        self.lessons(&mut out);
        count_lines(&out)
    }

    /// Quantas linhas as skills ocupam.
    fn skills_lines(&self) -> usize {
        let mut out = String::new();
        self.skills(&mut out);
        count_lines(&out)
    }

    /// Um evento: o código e o tipo no subtítulo, o texto original inteiro e
    /// os outros campos, um por linha.
    fn event(&self, out: &mut String, event: &SpecEvent) {
        let code = self.material.codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());
        let kind = self.t(&format!("page.type.{}", event.event_type));
        let _ = writeln!(out, "### {code} ({kind})\n");
        let text = event.str_field("text").map(str::trim).filter(|t| !t.is_empty());
        if let Some(text) = text {
            out.push_str(text);
            out.push('\n');
        }
        let fields = self.fields(event);
        if !fields.is_empty() {
            if text.is_some() {
                out.push('\n');
            }
            for line in fields {
                out.push_str(&line);
                out.push('\n');
            }
        }
        out.push('\n');
    }

    /// Os campos do evento fora do texto, na ordem em que o tipo os declara.
    /// As palavras-chave e o campo de busca nunca saem: o pedido mostra só o
    /// texto original de cada item. O número da onda também não se repete: ele
    /// já está no título do pedido.
    fn fields(&self, event: &SpecEvent) -> Vec<String> {
        let Some(spec) = type_spec(&event.event_type) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for field in spec.fields {
            if matches!(field.name, "text" | "keys" | "search")
                || matches!(
                    (event.event_type.as_str(), field.name),
                    ("wave", "n") | ("task" | "send" | "delivered", "wave")
                )
            {
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

    /// O material de uma onda com os itens combinados escolhidos para ela.
    fn with_agreed(log: &SpecLog, wave: u64) -> Material<'_> {
        let mut m = material(log, wave);
        m.agreed = agreed_for(log, wave);
        m
    }

    /// O item que a tarefa declara cobrir entra por inteiro; o resto do
    /// combinado entra como ponteiro de uma linha, com o código, o título e o
    /// comando que lê o item inteiro.
    #[test]
    fn the_covered_item_comes_whole_and_the_rest_comes_as_a_one_line_pointer() {
        let plan = plan();
        let prompt = build(&with_agreed(&plan, 1), Locale::PtBr).unwrap();
        // A onda 1 declara cobrir a regra da barra de status: ela vem inteira,
        // com o título dela e o exemplo.
        assert!(prompt.text.contains("### MSTD-RULE-0003"), "{}", prompt.text);
        assert!(prompt.text.contains("Exemplo: duas linhas"), "{}", prompt.text);
        // A regra do commit não é coberta por nenhuma tarefa: vira ponteiro, e
        // o exemplo dela não vai junto.
        assert!(!prompt.text.contains("### MSTD-RULE-0002"), "{}", prompt.text);
        assert!(!prompt.text.contains("título curto"), "{}", prompt.text);
        let pointer = prompt
            .text
            .lines()
            .find(|line| line.starts_with("- MSTD-RULE-0002"))
            .unwrap_or_else(|| panic!("sem ponteiro da regra do commit: {}", prompt.text));
        assert!(pointer.contains("O commit segue o modelo aprovado"), "{pointer}");
        assert!(
            pointer.contains("mustard-rt run read agreed --spec teste --term MSTD-RULE-0002"),
            "{pointer}"
        );
    }

    /// O item marcado como válido para todas as ondas entra por inteiro em
    /// cada uma delas, sem nenhuma tarefa precisar declará-lo.
    #[test]
    fn the_item_that_holds_for_every_wave_comes_whole_in_all_of_them() {
        let log = log(&[
            (
                "rule",
                json!({"text": "Nenhuma onda fecha com a suíte vermelha", "keys": ["suíte"],
                       "example": "a onda para", "applies_to": {"files": ["**"]}}),
            ),
            ("wave", json!({"n": 1, "text": "Leitura", "criteria": [], "done_when": "lê"})),
            ("task", json!({"wave": 1, "text": "Escrever o leitor", "files": [{"path": "src/a.rs"}]})),
            ("wave", json!({"n": 2, "text": "Página", "criteria": [], "done_when": "sai"})),
            ("task", json!({"wave": 2, "text": "Gravar a página", "files": [{"path": "src/b.rs"}]})),
        ]);
        for wave in [1, 2] {
            let prompt = build(&with_agreed(&log, wave), Locale::PtBr).unwrap();
            assert!(prompt.text.contains("Nenhuma onda fecha com a suíte vermelha"), "onda {wave}");
        }
    }

    /// O montador enche por ordem de relevância até o teto: o item coberto
    /// que não cabe vira ponteiro, e o pedido fica dentro do teto.
    #[test]
    fn the_covered_item_that_does_not_fit_becomes_a_pointer_too() {
        let huge = "uma linha da regra comprida\n".repeat(MAX_LINES);
        let log = log(&[
            ("rule", json!({"text": huge, "keys": ["comprida"], "example": "exemplo"})),
            ("rule", json!({"text": "A regra curta cabe", "keys": ["curta"], "example": "exemplo"})),
            ("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"})),
            (
                "task",
                json!({"wave": 1, "text": "Fazer", "files": [{"path": "src/a.rs"}], "covers": [1, 2]}),
            ),
        ]);
        let prompt = build(&with_agreed(&log, 1), Locale::PtBr).expect("cabe depois do recorte");
        assert!(prompt.lines <= MAX_LINES, "{} linhas", prompt.lines);
        assert!(prompt.text.contains("### MSTD-RULE-0002"), "{}", prompt.text);
        assert!(!prompt.text.contains("### MSTD-RULE-0001"), "a regra comprida ficou inteira");
        assert!(prompt.text.contains("--term MSTD-RULE-0001"), "{}", prompt.text);
    }

    /// Quando nem o que fica inteiro cabe, a recusa diz o que tirar: cada
    /// parte copiada por inteiro, com quantas linhas ela ocupa.
    #[test]
    fn a_request_that_still_does_not_fit_is_refused_saying_what_to_take_out() {
        let long = "uma linha da onda\n".repeat(MAX_LINES + 10);
        let log = log(&[
            ("rule", json!({"text": "Uma regra qualquer", "keys": ["regra"], "example": "exemplo"})),
            ("wave", json!({"n": 1, "text": long, "criteria": [], "done_when": "pronto"})),
        ]);
        let refused = build(&with_agreed(&log, 1), Locale::PtBr).unwrap_err();
        assert_eq!(refused.reason(), "wave-prompt-too-long");
        for lang in [Locale::PtBr, Locale::EnUs] {
            let message = build(&with_agreed(&log, 1), lang).unwrap_err().message(lang);
            assert!(message.contains(translate("prompt.part.wave", lang)), "{message}");
            assert!(!message.contains("{parts}"), "{message}");
        }
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
