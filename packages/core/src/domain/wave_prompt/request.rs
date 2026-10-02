//! O texto do pedido de uma onda e a lista dos itens que ele lista.
//!
//! As duas coisas saem da mesma passagem sobre o material ([`Listing`]): a
//! que imprime as linhas e a que diz, para a entrega conferir, quais itens o
//! agente precisa ter lido. Nenhuma calcula a lista por conta própria, então
//! o que o pedido mostra e o que a entrega cobra nunca discordam.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde_json::Value;

use super::{code_of, language_line, lowered, pattern_block_in_step, wave_title, Material, Writer, STEP_INDENT};
use crate::domain::spec_events::SpecEvent;

/// Uma tarefa da onda, com o que ela atende.
struct TaskStep<'a> {
    task: &'a SpecEvent,
    attended: Vec<&'a SpecEvent>,
}

/// Tudo o que o pedido de uma onda lista, na ordem em que o imprime. Cada
/// item aparece uma vez só: o que a onda cita de mais de um jeito fica no
/// primeiro lugar em que o pedido o mostra.
struct Listing<'a> {
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
        let fix = material.fix.iter().copied().filter(|item| fresh(item)).collect();
        let mut steps = Vec::new();
        for task in ordered_tasks(material) {
            if !fresh(task) {
                continue;
            }
            let mut attended = Vec::new();
            for item in attended_by(material, task) {
                if fresh(item) {
                    attended.push(item);
                }
            }
            steps.push(TaskStep { task, attended });
        }
        let obey = material.agreed.iter().copied().filter(|item| fresh(item)).collect();
        let lessons = material.lessons.clone();
        Self { fix, steps, obey, lessons }
    }

    /// Os itens da spec, na ordem em que o pedido os imprime.
    fn items(&self) -> impl Iterator<Item = &'a SpecEvent> + '_ {
        let steps = self.steps.iter().flat_map(|step| std::iter::once(step.task).chain(step.attended.iter().copied()));
        self.fix.iter().copied().chain(steps).chain(self.obey.iter().copied())
    }
}

/// O que o pedido da onda lista, para a entrega conferir a leitura: o código
/// de cada item da spec e `lesson-<número>` de cada lição, na ordem em que o
/// pedido os imprime. É a mesma lista que o texto do pedido percorre.
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
    let order: Vec<u64> =
        material.block.iter().find(|e| e.event_type == "wave").map(|wave| wave.ints("order")).unwrap_or_default();
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
        let _ = writeln!(
            out,
            "{}\n",
            self.t("prompt.model.wave")
                .replace("{model}", m.execution.requested_model())
                .replace("{effort}", m.execution.requested_effort())
        );
        let _ = writeln!(out, "{}\n", language_line(&m.execution.language));
        self.delivers(&mut out);
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.read"));
        self.read_example(&mut out, "prompt.read.wave", m.execution.copy.is_some());
        self.to_do(&mut out, &listing);
        self.to_obey(&mut out, &listing);
        self.to_return(&mut out);
        self.to_work(&mut out);
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
        let Some(wave) = self.material.block.iter().copied().find(|e| e.event_type == "wave") else { return };
        let done_when = wave.str_field("done_when").unwrap_or_default().trim();
        if done_when.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n\n{done_when}\n", self.t("prompt.part.delivers"));
    }

    /// "O que fazer": o conserto, quando a onda volta por reprovação, e os
    /// passos numerados — ler o que obedecer, cada tarefa, a suíte do projeto
    /// e a entrega. Sob cada tarefa vêm, recuados, o que ela atende, a ordem
    /// de ler a tarefa, os arquivos, o que ler antes, quem testa e o padrão
    /// do projeto.
    fn to_do(&self, out: &mut String, listing: &Listing) {
        let m = self.material;
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
        for at in &listing.steps {
            let pad = step(out, &self.t("prompt.step.task").replace("{item}", &self.item_tail(at.task)));
            self.task_lines(out, at, &pad);
        }
        if let Some(command) = &m.execution.test {
            step(out, &self.t("prompt.step.suite").replace("{command}", command));
        }
        step(out, &self.t("prompt.step.deliver").replace("{part}", self.t("prompt.part.return")));
        out.push('\n');
    }

    /// As linhas recuadas sob o passo de uma tarefa. `pad` é o recuo dos passos.
    fn task_lines(&self, out: &mut String, at: &TaskStep, pad: &str) {
        let m = self.material;
        for item in &at.attended {
            // A mensagem do usuário diz de quem ela é; os outros itens levam o
            // tipo de sempre, em minúscula, no meio da frase.
            let line = if item.event_type == "message" {
                format!("{} {}", self.t("prompt.step.user_message"), self.item_tail(item))
            } else {
                lowered(&self.item_text(item))
            };
            let _ = writeln!(out, "{pad}- {}", self.t("prompt.step.attends").replace("{item}", &line));
        }
        let only_message = at.attended.len() == 1 && at.attended[0].event_type == "message";
        let read = match (at.attended.is_empty(), only_message) {
            (true, _) => "prompt.step.read_task",
            (false, true) => "prompt.step.read_task_message",
            (false, false) => "prompt.step.read_task_attends",
        };
        let _ = writeln!(out, "{pad}- {}", self.t(read));
        let code = code_of(m, at.task);
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
            let _ = writeln!(out, "{pad}- {}", self.t(key).replace("{files}", &files.join(", ")));
        }
        if let Some((_, reads)) = m.task_reads.iter().find(|(task, _)| *task == code) {
            let hints: Vec<String> = reads.iter().map(|file| self.read_hint(file)).collect();
            let _ = writeln!(out, "{pad}- {}", self.t("prompt.step.read_before").replace("{hints}", &hints.join(", ")));
        }
        for path in paths {
            let Some(tests) = m.file_tests.get(path) else { continue };
            let list = tests.iter().map(|test| format!("`{test}`")).collect::<Vec<_>>().join(", ");
            let line = self.t("prompt.task.tested_by").replace("{file}", path).replace("{tests}", &list);
            let _ = writeln!(out, "{pad}- {line}");
        }
        if let Some(pattern) = m.task_patterns.get(&code) {
            out.push_str(&pattern_block_in_step(pattern, self.lang));
        }
    }

    /// "O que obedecer": as regras e decisões que valem para a onda e as
    /// lições dos arquivos dela, uma linha cada; sem lição, a frase que diz
    /// isso. As skills que as tarefas nomeiam vêm por último, cada uma como
    /// recomendação de uma linha.
    fn to_obey(&self, out: &mut String, listing: &Listing) {
        let m = self.material;
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
        if !m.skills.is_empty() {
            let _ = writeln!(out, "\n{}", self.t("prompt.skill.read"));
            for skill in &m.skills {
                let _ = write!(out, "- **{}**", skill.name);
                if skill.stale {
                    let _ = write!(out, " ({})", self.t("prompt.skill.stale"));
                }
                if !skill.when.trim().is_empty() {
                    let _ = write!(out, " — {}", skill.when.trim());
                }
                let _ = writeln!(out, " — `{}`", skill.path);
            }
        }
        out.push('\n');
    }

    /// "O que devolver": a entrega pela ferramenta, o campo `commit` e a
    /// ordem de não deixar texto solto fora da entrega.
    fn to_return(&self, out: &mut String) {
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.return"));
        let _ = writeln!(out, "- {}", self.t("prompt.execution.report_lines"));
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
