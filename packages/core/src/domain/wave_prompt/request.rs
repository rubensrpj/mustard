//! O texto do pedido de uma onda, o bloco de cada tarefa e a lista dos itens
//! que ele lista.
//!
//! As três coisas saem da mesma passagem sobre o material ([`Listing`]): a
//! que imprime as linhas, a que monta o bloco de cada tarefa e a que diz,
//! para a entrega conferir, quais itens o agente precisa ter lido. Nenhuma
//! calcula a lista por conta própria, então o que o pedido mostra e o que a
//! entrega cobra nunca discordam. O pedido traz inteiro só o bloco da
//! primeira tarefa; o de cada uma das outras chega na resposta do passo que
//! termina a anterior.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde_json::Value;

use super::{
    Material, STEP_INDENT, TaskBlock, Writer, code_of, item_body, language_line, lowered,
    pattern_block_in_task, wave_title,
};
use crate::domain::spec_events::SpecEvent;

/// Uma tarefa da onda, com o que ela atende.
struct TaskStep<'a> {
    task: &'a SpecEvent,
    /// O que ela atende e o pedido mostra pela primeira vez nela: o bloco
    /// dela traz o texto inteiro.
    attended: Vec<&'a SpecEvent>,
    /// O que ela atende e já veio antes no pedido: o bloco o cita numa linha.
    again: Vec<&'a SpecEvent>,
}

/// Tudo o que o pedido de uma onda lista, na ordem em que o imprime. Cada
/// item aparece uma vez só: o que a onda cita de mais de um jeito fica no
/// primeiro lugar em que o pedido o mostra.
struct Listing<'a> {
    /// O resumo que a onda continua, o primeiro item do pedido.
    summary: Option<&'a SpecEvent>,
    /// As linhas do conserto, quando a onda volta por reprovação.
    fix: Vec<&'a SpecEvent>,
    /// As tarefas, na ordem de execução, cada uma com o que ela atende.
    steps: Vec<TaskStep<'a>>,
    /// As regras e decisões que valem para a onda.
    obey: Vec<&'a SpecEvent>,
    /// As lições dos arquivos da onda.
    lessons: Vec<&'a SpecEvent>,
}

impl<'a> Listing<'a> {
    fn of(material: &Material<'a>) -> Self {
        let mut seen: BTreeSet<u64> = BTreeSet::new();
        let mut fresh = |item: &SpecEvent| seen.insert(item.id);
        let summary = material.summary.filter(|item| fresh(item));
        let fix = material.fix.iter().copied().filter(|item| fresh(item)).collect();
        let mut steps = Vec::new();
        for task in ordered_tasks(material) {
            if !fresh(task) {
                continue;
            }
            let (mut attended, mut again) = (Vec::new(), Vec::new());
            for item in attended_by(material, task) {
                if fresh(item) {
                    attended.push(item);
                } else {
                    again.push(item);
                }
            }
            steps.push(TaskStep { task, attended, again });
        }
        // A mesma conta com que a entrega da onda é cobrada, sobre a lista
        // que o envio grava do que o pedido lê.
        let obey = super::carried_agreed(material.agreed.iter().copied()).into_iter().filter(|item| fresh(item)).collect();
        let lessons = material.lessons.clone();
        Self { summary, fix, steps, obey, lessons }
    }

    /// Os itens da spec, na ordem em que o pedido os imprime.
    fn items(&self) -> impl Iterator<Item = &'a SpecEvent> + '_ {
        let steps = self.steps.iter().flat_map(|step| std::iter::once(step.task).chain(step.attended.iter().copied()));
        self.summary.into_iter().chain(self.fix.iter().copied()).chain(steps).chain(self.obey.iter().copied())
    }
}

/// O que o pedido da onda lista, para a entrega conferir a leitura: o código
/// de cada item da spec — o resumo que a onda continua primeiro — e
/// `lesson-<número>` de cada lição, na ordem em que o pedido os imprime. É a
/// mesma lista que o texto do pedido percorre.
#[must_use]
pub fn listed(material: &Material) -> Vec<String> {
    let listing = Listing::of(material);
    let mut out: Vec<String> = listing.items().map(|item| code_of(material, item)).collect();
    out.extend(listing.lessons.iter().map(|lesson| format!("lesson-{}", lesson.id)));
    out
}

/// As tarefas da onda na ordem de execução que ela declara: primeiro as que
/// a onda lista, na ordem em que ela as lista, depois o que sobrou, na ordem
/// do arquivo. A onda que não declara ordem sai como está no arquivo.
fn ordered_tasks<'a>(material: &Material<'a>) -> Vec<&'a SpecEvent> {
    let order: Vec<u64> = material.block.iter().find(|e| e.event_type == "wave").map(|wave| wave.ints("order")).unwrap_or_default();
    let mut out: Vec<&SpecEvent> = Vec::new();
    for id in &order {
        if let Some(event) = material.block.iter().copied().find(|e| e.id == *id) {
            out.push(event);
        }
    }
    for event in &material.block {
        if !out.iter().any(|had| had.id == event.id) {
            out.push(event);
        }
    }
    out.retain(|event| event.event_type == "task");
    out
}

/// O que a tarefa atende, na ordem em que ela cita: primeiro o que cobre
/// (`covers`), depois a mensagem de onde nasceu (`origin`), sem repetir.
fn attended_by<'a>(material: &Material<'a>, task: &SpecEvent) -> Vec<&'a SpecEvent> {
    let cited = task.ints("covers").into_iter().chain(task.int("origin"));
    let mut out: Vec<&SpecEvent> = Vec::new();
    for id in cited {
        if let Some(item) = material.attended.get(&id).copied()
            && !out.iter().any(|had| had.id == item.id)
        {
            out.push(item);
        }
    }
    out
}

impl Writer<'_> {
    /// O pedido da onda: o título, o modelo, os idiomas e as seis seções, sempre
    /// na mesma ordem.
    pub(super) fn text(&self) -> String {
        let m = self.material;
        let listing = Listing::of(m);
        let mut out = String::new();
        let _ = writeln!(out, "{}\n", wave_title(&m.spec, m.wave, self.lang));
        // Logo depois do título, que segue sozinho na primeira linha, porque
        // é por ele que os ganchos acham a onda.
        if let Some(copy) = &m.execution.copy {
            let _ = writeln!(out, "{}\n", self.t("prompt.copy.first").replace("{copy}", &copy.path));
        }
        let _ = writeln!(
            out,
            "{}\n",
            self.t("prompt.model.wave").replace("{model}", m.execution.requested_model()).replace("{effort}", m.execution.requested_effort())
        );
        let _ = writeln!(out, "{}\n", language_line(&m.execution.language));
        self.started(&mut out, listing.summary);
        self.delivers(&mut out);
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.read"));
        self.read_example(&mut out, "prompt.read.wave", m.execution.copy.is_some());
        self.to_do(&mut out, &listing);
        self.to_obey(&mut out, &listing);
        self.to_return(&mut out);
        self.to_work(&mut out);
        self.prepared_sources(&mut out);
        if let Some(section) = super::project_rules_section(&m.project_rules, self.lang) {
            let _ = writeln!(out, "\n{section}\n");
        }
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }

    /// O que a onda entrega: a frase do `done_when` do evento da onda, como
    /// foi gravada — a prova dos critérios ou, sem prova, os títulos das
    /// tarefas —, porque é o que abre o trabalho. Sem onda no material ou
    /// sem a frase, a seção não aparece.
    fn delivers(&self, out: &mut String) {
        let Some(wave) = self.material.block.iter().copied().find(|e| e.event_type == "wave") else {
            return;
        };
        let done_when = wave.str_field("done_when").unwrap_or_default().trim();
        if done_when.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n\n{done_when}\n", self.t("prompt.part.delivers"));
    }

    /// "O que fazer": o conserto, quando a onda volta por reprovação, e os
    /// passos numerados — ler o que obedecer, fazer as tarefas uma por vez e
    /// gravar a entrega. As tarefas vêm só pelo código e pelo título, na
    /// ordem, com a primeira marcada; o bloco inteiro dela vem logo depois
    /// dos passos, e depois dele os princípios do produto que o agente de onda
    /// recebe. A suíte inteira do projeto não é passo do agente: quem a roda,
    /// com o lint, é a rodada, antes do commit.
    fn to_do(&self, out: &mut String, listing: &Listing) {
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.do"));
        if !listing.fix.is_empty() {
            let _ = writeln!(out, "{}\n", self.t("prompt.fix.wave"));
            for item in &listing.fix {
                let _ = writeln!(out, "- {}", self.item_text(item));
            }
            out.push('\n');
        }
        let mut number = 0;
        let mut step = |out: &mut String, text: &str| -> String {
            number += 1;
            let _ = writeln!(out, "{number}. {text}");
            " ".repeat(STEP_INDENT)
        };
        if !listing.obey.is_empty() || !listing.lessons.is_empty() {
            let part = self.t("prompt.part.obey");
            step(out, &self.t("prompt.step.read").replace("{part}", part));
        }
        if !listing.steps.is_empty() {
            let pad = step(out, self.t("prompt.step.tasks"));
            for (n, at) in listing.steps.iter().enumerate() {
                let item = self.item_tail(at.task);
                let line = if n == 0 { self.t("prompt.step.first").replace("{item}", &item) } else { item };
                let _ = writeln!(out, "{pad}- {line}");
            }
        }
        step(out, &self.t("prompt.step.deliver").replace("{part}", self.t("prompt.part.return")));
        out.push('\n');
        if let Some(first) = listing.steps.first() {
            let _ = writeln!(out, "{}\n", self.task_block(first, 1, listing.steps.len()).text);
        }
    }

    /// O bloco de cada tarefa, na ordem de execução ([`super::task_blocks`]).
    pub(super) fn task_blocks(&self) -> Vec<TaskBlock> {
        let listing = Listing::of(self.material);
        let total = listing.steps.len();
        listing.steps.iter().enumerate().map(|(n, at)| self.task_block(at, n + 1, total)).collect()
    }

    /// O bloco da tarefa `at`, a `n`-ésima de `total`: o título e o código, o
    /// texto inteiro dela e de cada item que ela atende, os arquivos, o que
    /// mudou neles depois do texto, o que ler antes, quem testa, o padrão do
    /// projeto e as skills dela, citadas pelo nome e pelo caminho. O que ela
    /// atende e já veio antes no pedido entra numa linha.
    fn task_block(&self, at: &TaskStep, n: usize, total: usize) -> TaskBlock {
        let m = self.material;
        let code = code_of(m, at.task);
        let mut out = String::new();
        let heading = self
            .t("prompt.task.heading")
            .replace("{n}", &n.to_string())
            .replace("{total}", &total.to_string())
            .replace("{item}", &self.item_tail(at.task));
        let _ = writeln!(out, "### {heading}\n");
        self.whole_text(&mut out, at.task);
        for item in &at.attended {
            let _ = writeln!(out, "#### {}\n", self.t("prompt.step.attends").replace("{item}", &self.attends_line(item)));
            self.whole_text(&mut out, item);
        }
        let mut lines = String::new();
        for item in &at.again {
            let _ = writeln!(lines, "- {}", self.t("prompt.step.attends").replace("{item}", &self.attends_line(item)));
        }
        let paths: Vec<&str> = at
            .task
            .fields
            .get("files")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|file| file.get("path").and_then(Value::as_str))
            .collect();
        if !paths.is_empty() {
            let key = if paths.len() == 1 { "prompt.step.file" } else { "prompt.step.files" };
            let files: Vec<String> = paths.iter().map(|path| format!("`{path}`")).collect();
            let _ = writeln!(lines, "- {}", self.t(key).replace("{files}", &files.join(", ")));
        }
        self.changed_since_text(&mut lines, &code, "");
        if let Some((_, reads)) = m.task_reads.iter().find(|(task, _)| *task == code) {
            let hints: Vec<String> = reads.iter().map(|file| self.read_hint(file)).collect();
            let _ = writeln!(lines, "- {}", self.t("prompt.step.read_before").replace("{hints}", &hints.join(", ")));
        }
        for path in paths {
            let Some(tests) = m.file_tests.get(path) else {
                continue;
            };
            let list = tests.iter().map(|test| format!("`{test}`")).collect::<Vec<_>>().join(", ");
            let line = self.t("prompt.task.tested_by").replace("{file}", path).replace("{tests}", &list);
            let _ = writeln!(lines, "- {line}");
        }
        if let Some(pattern) = m.task_patterns.get(&code) {
            lines.push_str(&pattern_block_in_task(pattern, self.lang));
        }
        if !lines.is_empty() {
            let _ = writeln!(out, "{lines}");
        }
        let skills = m.task_skills.get(&code).map(Vec::as_slice).unwrap_or_default();
        for skill in skills {
            let mut line = self.t("prompt.task.skill").replace("{name}", &skill.name).replace("{path}", &skill.path);
            if skill.stale {
                let _ = write!(line, " ({})", self.t("prompt.skill.stale"));
            }
            let _ = writeln!(out, "- {line}. {}", self.t("prompt.task.skill_read"));
            if !skill.note.trim().is_empty() {
                let _ = writeln!(out, "\n{}\n", skill.note.trim());
            }
        }
        let mut items = vec![code.clone()];
        items.extend(at.attended.iter().map(|item| code_of(m, item)));
        let skills = skills.iter().map(|skill| skill.name.clone()).collect();
        TaskBlock { task: code, text: out.trim_end().to_string(), items, skills }
    }

    /// O texto inteiro de `item` ([`item_body`]), seguido de uma linha em
    /// branco; sem texto, nada.
    fn whole_text(&self, out: &mut String, item: &SpecEvent) {
        let body = item_body(item, &code_of(self.material, item), self.lang);
        if !body.trim().is_empty() {
            let _ = writeln!(out, "{}\n", body.trim_end());
        }
    }

    /// O que vem depois de "Atende:": a mensagem do usuário diz de quem ela
    /// é; os outros itens levam o tipo de sempre, em minúscula, no meio da
    /// frase.
    fn attends_line(&self, item: &SpecEvent) -> String {
        if item.event_type == "message" {
            format!("{} {}", self.t("prompt.step.user_message"), self.item_tail(item))
        } else {
            lowered(&self.item_text(item))
        }
    }

    /// "O que obedecer": as regras e decisões que valem para a onda e as
    /// lições dos arquivos dela, uma linha cada; sem lição, a frase que diz
    /// isso. As skills vêm inteiras no bloco da tarefa que as usa.
    fn to_obey(&self, out: &mut String, listing: &Listing) {
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.obey"));
        for item in &listing.obey {
            let _ = writeln!(out, "- {}", self.item_text(item));
        }
        for lesson in &listing.lessons {
            let _ = writeln!(out, "- {}", self.lesson_text(lesson));
        }
        if listing.lessons.is_empty() {
            let _ = writeln!(out, "- {}", self.t("prompt.obey.no_lessons"));
        }
        out.push('\n');
    }

    /// "O que devolver": o comando pronto do passo, a entrega pela
    /// ferramenta, com o comando pronto dela, o campo `commit` e a ordem de
    /// não deixar texto solto fora da entrega.
    fn to_return(&self, out: &mut String) {
        let m = self.material;
        let command = |key: &str| {
            self.t(key).replace("{root}", &self.root_flag(m.execution.copy.is_some())).replace("{spec}", &m.spec).replace("{wave}", &m.wave.to_string())
        };
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.return"));
        let _ = writeln!(out, "- {}", command("prompt.return.step"));
        let _ = writeln!(out, "- {}", self.t("prompt.execution.report_lines"));
        let _ = writeln!(out, "- {}", command("prompt.return.delivered"));
        let _ = writeln!(out, "- {}", self.t("prompt.execution.commit_field"));
        let _ = writeln!(out, "- {}", self.t("prompt.return.loose"));
        out.push('\n');
    }

    /// "Como trabalhar": o que carrega valor deste projeto e desta rodada — a
    /// cópia separada, o preparo que o projeto declara, o comando de compilar
    /// e as outras ondas em andamento, com os arquivos delas — e, ligadas à
    /// cópia, a frase que diz com todas as letras que o agente não comita. O
    /// comando de testar mora em "O que fazer". Sem nada disso, a seção não
    /// aparece.
    fn to_work(&self, out: &mut String) {
        let execution = &self.material.execution;
        let mut body = String::new();
        if let Some(copy) = &execution.copy {
            let line = self.t("prompt.execution.copy").replace("{copy}", &copy.path).replace("{root}", &execution.root);
            let _ = writeln!(body, "- {line}");
            self.prepare(&mut body, Some(copy));
            let _ = writeln!(body, "- {}", self.t("prompt.execution.no_commit"));
        }
        self.build_line(&mut body);
        if !execution.running.is_empty() {
            let _ = writeln!(body, "- {}", self.t("prompt.execution.running"));
        }
        for (wave, files) in &execution.running {
            let name = self.t("prompt.execution.wave").replace("{n}", &wave.to_string());
            let files: Vec<String> = files.iter().map(|file| format!("`{file}`")).collect();
            if files.is_empty() {
                let _ = writeln!(body, "  - {name}");
            } else {
                let _ = writeln!(body, "  - {name}: {}", files.join(", "));
            }
        }
        if body.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n\n{body}", self.t("prompt.part.work"));
    }
}
