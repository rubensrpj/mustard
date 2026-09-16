//! O pedido de uma onda: o texto que o agente dela recebe, montado dos blocos
//! já lidos do arquivo de eventos.
//!
//! Tudo aqui é puro: sem disco, sem relógio e sem caminho da máquina. Quem lê
//! o arquivo, o banco de lições e os arquivos das skills entrega os blocos
//! prontos em [`Material`]; esta função só os escreve, sempre na mesma ordem,
//! então o mesmo material dá sempre os mesmos bytes.
//!
//! O pedido leva a lista, não o texto. Nenhum item é copiado: cada um entra
//! como uma linha com o número, o tipo e o comando que o lê pelo binário, e o
//! agente da onda lê o que precisa na hora de agir. Os itens da onda saem na
//! ordem de execução que ela declara; sem essa ordem, na ordem do arquivo. A
//! lista inteira fica no pedido, porque é ela que mostra o escopo todo de uma
//! vez. As lições entram pelo texto — elas vêm do banco, não da spec — e cada
//! skill entra como recomendação de uma linha. O teto de [`MAX_LINES`] linhas
//! continua conferido aqui, e passa a sobrar.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde_json::Value;

use crate::domain::lessons::{applies_to, Scope};
use crate::domain::search;
use crate::domain::spec_events::{Block, BlockQuery, Refusal, SpecEvent, SpecLog};
use crate::platform::i18n::{translate, Locale};

/// O teto de linhas de um pedido de onda.
pub const MAX_LINES: usize = 500;

/// A skill que uma tarefa da onda nomeia, recomendada no pedido. O texto dela
/// não entra: a skill mora num arquivo do projeto, e o agente da onda o lê.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// O nome pelo qual a tarefa a chama.
    pub name: String,
    /// Quando usar, na própria descrição da skill.
    pub when: String,
    /// O caminho do arquivo, a partir da raiz do projeto.
    pub path: String,
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
    /// Os defeitos já vistos nos arquivos da onda, que só o pedido do revisor
    /// leva: o erro que já aconteceu ali é o que tem mais chance de voltar.
    pub defects: Vec<&'a SpecEvent>,
    /// As skills nomeadas pelas tarefas, na ordem dos nomes.
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

/// A recusa do teto de linhas, com as partes do pedido e o tamanho de cada
/// uma: quem divide a onda precisa saber de onde vêm as linhas.
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
    Writer { material, lang }.text()
}

/// O texto do pedido do revisor da onda: a mesma lista de itens e os
/// critérios que ele confere, mais os defeitos já vistos nos arquivos da onda.
#[must_use]
pub fn write_review(material: &Material, lang: Locale) -> String {
    Writer { material, lang }.review_text()
}

/// As partes do pedido, cada uma com quantas linhas ocupa, separadas por
/// vírgula.
fn parts(material: &Material, lang: Locale) -> String {
    let w = Writer { material, lang };
    let mut out: Vec<String> = Vec::new();
    let mut named = |key: &str, lines: usize| {
        if lines > 0 {
            out.push(format!("{} ({lines})", translate(key, lang)));
        }
    };
    named("prompt.part.specification", w.part_lines("prompt.part.specification", &material.specification));
    named("prompt.part.agreed", w.part_lines("prompt.part.agreed", &material.agreed));
    named("prompt.part.wave", w.part_lines("prompt.part.wave", &w.wave_items()));
    named("prompt.part.criteria", w.part_lines("prompt.part.criteria", &material.criteria));
    named("prompt.part.lessons", w.lessons_lines());
    named("prompt.part.skills", w.skills_lines());
    named("prompt.part.delivered", w.part_lines("prompt.part.delivered", &material.delivered));
    out.join(", ")
}

// ---------------------------------------------------------------------------
// A linha de cada item
// ---------------------------------------------------------------------------

/// A linha de um item no pedido: o código, o tipo em palavras e o comando que
/// lê o item inteiro pelo binário. Nenhum texto do item entra aqui.
fn pointer(material: &Material, item: &SpecEvent, lang: Locale) -> String {
    let code = material.codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string());
    let kind = translate(&format!("page.type.{}", item.event_type), lang);
    let block = item.block().map_or("waves", Block::name);
    format!("- {code} ({kind}) — `mustard-rt run read {block} --spec {} --term {code}`", material.spec)
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
        self.part(&mut out, "prompt.part.wave", &self.wave_items());
        self.part(&mut out, "prompt.part.criteria", &m.criteria);
        self.lessons(&mut out);
        self.skills(&mut out);
        self.part(&mut out, "prompt.part.delivered", &m.delivered);
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }

    /// O pedido do revisor: as instruções fixas dele, a lista de itens da
    /// onda, os critérios que ele confere e os defeitos já vistos naqueles
    /// arquivos. Nenhum texto de item é copiado aqui tampouco.
    fn review_text(&self) -> String {
        let m = self.material;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "# {}\n",
            self.t("prompt.review.title").replace("{spec}", &m.spec).replace("{n}", &m.wave.to_string())
        );
        out.push_str(self.t("prompt.review.fixed"));
        out.push_str("\n\n");
        self.part(&mut out, "prompt.part.wave", &self.wave_items());
        self.part(&mut out, "prompt.part.criteria", &m.criteria);
        self.defects(&mut out);
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }

    /// Os defeitos já vistos nos arquivos da onda: o texto original de cada
    /// um, como as lições. Eles vêm do banco, fora da spec, e não têm número
    /// para serem lidos depois.
    fn defects(&self, out: &mut String) {
        if self.material.defects.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.defects"));
        for defect in &self.material.defects {
            let text = defect.str_field("text").unwrap_or_default().trim();
            let _ = writeln!(out, "- {text}");
        }
        out.push('\n');
    }

    /// Os itens da onda na ordem de execução que ela declara: primeiro os que
    /// a onda lista, na ordem em que ela os lista, depois o que sobrou, na
    /// ordem do arquivo. A onda que não declara ordem sai como está no
    /// arquivo.
    fn wave_items(&self) -> Vec<&SpecEvent> {
        let order: Vec<u64> = self
            .material
            .block
            .iter()
            .find(|e| e.event_type == "wave")
            .map(|wave| wave.ints("order"))
            .unwrap_or_default();
        let mut out: Vec<&SpecEvent> = Vec::new();
        for id in &order {
            if let Some(event) = self.material.block.iter().copied().find(|e| e.id == *id) {
                out.push(event);
            }
        }
        for event in &self.material.block {
            if !out.iter().any(|had| had.id == event.id) {
                out.push(event);
            }
        }
        out
    }

    /// Uma parte do pedido: o título e uma linha por item, cada uma com o
    /// código, o tipo e o comando que lê o item. A parte sem nenhum item não
    /// aparece.
    fn part(&self, out: &mut String, key: &str, events: &[&SpecEvent]) {
        if events.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n", self.t(key));
        for event in events {
            let _ = writeln!(out, "{}", pointer(self.material, event, self.lang));
        }
        out.push('\n');
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

    /// As lições: só o texto original de cada uma, nunca o campo de busca.
    /// A lição vem do banco, fora da spec, e não tem número para ser lida
    /// depois.
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

    /// Uma linha por skill que uma tarefa nomeia: o nome, o quando usar e o
    /// caminho do arquivo. O texto não vem junto — o agente da onda lê a skill
    /// no disco pelo caminho recomendado aqui.
    fn skills(&self, out: &mut String) {
        if self.material.skills.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.skills"));
        let _ = writeln!(out, "{}\n", self.t("prompt.skill.read"));
        for skill in &self.material.skills {
            let _ = write!(out, "- **{}**", skill.name);
            if skill.stale {
                let _ = write!(out, " ({})", self.t("prompt.skill.stale"));
            }
            if !skill.when.trim().is_empty() {
                let _ = write!(out, " — {}", skill.when.trim());
            }
            let _ = writeln!(out, " — `{}`", skill.path);
        }
        out.push('\n');
    }
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

    /// O pedido leva a lista, não o texto: uma linha por item, com o número,
    /// o tipo em palavras e o comando que lê o item pelo binário. Nenhum
    /// texto de item é copiado.
    #[test]
    fn a_request_carries_one_line_per_item_with_its_number_type_and_command() {
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
        for text in ["Primeira onda", "Escrever o motor", "a suíte passa", "src/a.rs", "cargo test"] {
            assert!(!prompt.text.contains(text), "{text:?} foi copiado: {}", prompt.text);
        }
        assert!(
            prompt.text.contains(
                "- MSTD-TASK-0001 (tarefa) — `mustard-rt run read waves --spec teste --term MSTD-TASK-0001`"
            ),
            "{}",
            prompt.text
        );
        assert!(
            prompt.text.contains(
                "- MSTD-CRIT-0001 (critério) — `mustard-rt run read criteria --spec teste --term MSTD-CRIT-0001`"
            ),
            "{}",
            prompt.text
        );
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

    /// Um pedido acima do teto de linhas continua sendo recusado no mesmo
    /// lugar, e a mensagem diz quantas linhas ele tem e qual é o teto. Com a
    /// lista no lugar do texto, só uma onda com centenas de itens chega lá.
    #[test]
    fn a_request_over_the_line_limit_is_refused_saying_how_far_it_went() {
        let mut events: Vec<(&str, Value)> =
            vec![("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))];
        for _ in 0..=MAX_LINES {
            events.push(("task", json!({"wave": 1, "text": "Fazer", "files": [{"path": "src/a.rs"}]})));
        }
        let log = log(&events);
        let refused = build(&material(&log, 1), Locale::PtBr).unwrap_err();
        assert_eq!(refused.reason(), "wave-prompt-too-long");
        let message = refused.message(Locale::PtBr);
        assert!(message.contains(&MAX_LINES.to_string()), "{message}");
        let written = count_lines(&write(&material(&log, 1), Locale::PtBr));
        assert!(written > MAX_LINES);
        assert!(message.contains(&written.to_string()), "{message}");
        assert!(!refused.message(Locale::EnUs).is_empty());
        // A página ainda mostra o pedido grande: é ele que precisa ser visto.
        assert!(write(&material(&log, 1), Locale::PtBr).contains("--term MSTD-TASK-0001"));
    }

    /// Cada skill nomeada entra no pedido como uma linha — nome, quando usar e
    /// o caminho do arquivo —, sem o texto dela, e a skill cujo exemplo mudou
    /// depois dela sai marcada como a revisar.
    #[test]
    fn a_named_skill_is_recommended_by_path_and_a_stale_one_is_marked() {
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        let mut m = material(&log, 1);
        m.skills = vec![
            Skill {
                name: "add-run-command".into(),
                when: "acrescentar um comando run".into(),
                path: "apps/rt/.claude/skills/add-run-command/SKILL.md".into(),
                stale: false,
            },
            Skill {
                name: "add-hook-rule".into(),
                when: "acrescentar uma regra de gancho".into(),
                path: ".claude/skills/add-hook-rule/SKILL.md".into(),
                stale: true,
            },
        ];
        let prompt = build(&m, Locale::PtBr).unwrap();
        assert!(
            prompt.text.contains("`apps/rt/.claude/skills/add-run-command/SKILL.md`"),
            "{}",
            prompt.text
        );
        assert!(prompt.text.contains("acrescentar um comando run"), "{}", prompt.text);
        assert!(prompt.text.contains(translate("prompt.skill.read", Locale::PtBr)), "{}", prompt.text);
        let stale = translate("prompt.skill.stale", Locale::PtBr);
        assert!(prompt.text.contains(&format!("**add-hook-rule** ({stale})")), "{}", prompt.text);
        assert!(!prompt.text.contains(&format!("**add-run-command** ({stale})")), "{}", prompt.text);
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

    /// O item combinado escolhido para a onda entra como linha, com o número
    /// e o comando que o lê; o texto e o exemplo dele nunca entram.
    #[test]
    fn every_agreed_item_comes_as_a_line_and_never_as_text() {
        let plan = plan();
        let prompt = build(&with_agreed(&plan, 1), Locale::PtBr).unwrap();
        for text in ["O commit segue o modelo aprovado", "título curto", "duas linhas"] {
            assert!(!prompt.text.contains(text), "{text:?} foi copiado: {}", prompt.text);
        }
        let line = prompt
            .text
            .lines()
            .find(|line| line.starts_with("- MSTD-RULE-0002"))
            .unwrap_or_else(|| panic!("sem a linha da regra do commit: {}", prompt.text));
        assert!(line.contains("(regra)"), "{line}");
        assert!(line.contains("mustard-rt run read agreed --spec teste --term MSTD-RULE-0002"), "{line}");
    }

    /// O item marcado como válido para todas as ondas entra na lista de cada
    /// uma delas, sem nenhuma tarefa precisar declará-lo.
    #[test]
    fn the_item_that_holds_for_every_wave_is_listed_in_all_of_them() {
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
            assert!(prompt.text.contains("--term MSTD-RULE-0001"), "onda {wave}: {}", prompt.text);
            assert!(!prompt.text.contains("suíte vermelha"), "onda {wave}: {}", prompt.text);
        }
    }

    /// Os itens da onda saem na ordem de execução que ela declara; o que ela
    /// não lista vem depois, na ordem do arquivo. A onda sem essa ordem sai
    /// como está no arquivo.
    #[test]
    fn the_wave_items_come_in_the_execution_order_the_wave_declares() {
        let events = |order: Value| {
            vec![
                ("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto", "order": order})),
                ("task", json!({"wave": 1, "text": "Primeira", "files": [{"path": "src/a.rs"}]})),
                ("task", json!({"wave": 1, "text": "Segunda", "files": [{"path": "src/b.rs"}]})),
                ("task", json!({"wave": 1, "text": "Terceira", "files": [{"path": "src/c.rs"}]})),
            ]
        };
        let codes_in_order = |prompt: &str| -> Vec<String> {
            prompt
                .lines()
                .filter_map(|line| line.strip_prefix("- MSTD-TASK-"))
                .filter_map(|rest| rest.split(' ').next().map(str::to_string))
                .collect()
        };

        let declared = log(&events(json!([4, 2])));
        let prompt = build(&material(&declared, 1), Locale::PtBr).unwrap();
        assert_eq!(codes_in_order(&prompt.text), ["0003", "0001", "0002"], "{}", prompt.text);

        let plain = log(&events(json!([])));
        let prompt = build(&material(&plain, 1), Locale::PtBr).unwrap();
        assert_eq!(codes_in_order(&prompt.text), ["0001", "0002", "0003"], "{}", prompt.text);
    }

    /// Quando o pedido não cabe, a recusa diz de onde vêm as linhas: cada
    /// parte, com quantas ela ocupa.
    #[test]
    fn a_request_that_does_not_fit_is_refused_saying_where_the_lines_come_from() {
        let mut events: Vec<(&str, Value)> = vec![
            ("rule", json!({"text": "Uma regra qualquer", "keys": ["regra"], "example": "exemplo"})),
            ("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"})),
        ];
        for _ in 0..=MAX_LINES {
            events.push(("task", json!({"wave": 1, "text": "Fazer", "files": [{"path": "src/a.rs"}]})));
        }
        let log = log(&events);
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

    /// As instruções fixas dizem que ler o item pelo número é parte do
    /// trabalho, no lugar da frase que mandava nunca procurar o resto em
    /// outro arquivo. A proibição que sobra é só a de sair caçando o conteúdo
    /// em outro arquivo do projeto.
    #[test]
    fn the_fixed_instructions_say_that_reading_the_item_by_its_number_is_part_of_the_work() {
        for (lang, reading, forbidden) in [
            (Locale::PtBr, "Ler o item pelo número é parte do trabalho", "nunca vá procurar o resto em outro arquivo"),
            (Locale::EnUs, "Reading the item by its number is part of the work", "never go looking for the rest in another file"),
        ] {
            let fixed = translate("prompt.fixed", lang);
            assert!(fixed.contains(reading), "{fixed}");
            assert!(!fixed.contains(forbidden), "{fixed}");
        }
    }
}
