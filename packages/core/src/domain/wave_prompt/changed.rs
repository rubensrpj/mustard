//! O aviso do arquivo da tarefa que mudou depois do texto dela: quando um
//! commit tocou o arquivo de uma tarefa depois que o texto dela foi escrito,
//! ela pode já estar feita ou ter mudado. O pedido da onda traz, sob a
//! tarefa, uma linha com os commits e os arquivos, que manda o agente
//! conferir no código antes de mudar e, se o que a tarefa pede já está
//! feito, dizer isso na entrega e não mudar nada.
//!
//! Quem lê o git e acha os commits é quem monta o material
//! (`io::wave_prompt`); aqui a linha só se escreve, pelo código da tarefa.

use std::fmt::Write as _;

use super::Writer;

/// O que mudou nos arquivos de uma tarefa depois do texto dela: os commits e
/// os arquivos da tarefa que eles tocaram, nunca um arquivo de fora dela.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskChange {
    /// Os commits, do mais novo ao mais velho, pelo hash curto.
    pub commits: Vec<String>,
    /// Os arquivos da tarefa que esses commits tocaram.
    pub files: Vec<String>,
}

impl Writer<'_> {
    /// A linha da tarefa de código `code` com o aviso, recuada em `pad`, logo
    /// depois da linha dos arquivos. Sem mudança para a tarefa, nada.
    pub(super) fn changed_since_text(&self, out: &mut String, code: &str, pad: &str) {
        let Some(change) = self.material.task_changes.get(code) else { return };
        let files: Vec<String> = change.files.iter().map(|file| format!("`{file}`")).collect();
        let line = self
            .t("wave_prompt.task_changed")
            .replace("{commits}", &change.commits.join(", "))
            .replace("{files}", &files.join(", "));
        let _ = writeln!(out, "{pad}- {line}");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::domain::spec_events::{normalize, parse_log, render_line, stamp, SpecLog};
    use crate::domain::wave_prompt::{write, Material};
    use crate::platform::i18n::Locale;

    /// A onda 1, com a tarefa que muda `src/a.rs`.
    fn plan() -> SpecLog {
        let events = [
            ("wave", json!({"n": 1, "text": "Uma.", "criteria": [], "done_when": "pronto"})),
            ("task", json!({"wave": 1, "text": "Somar o dia.", "files": [{"path": "src/a.rs"}]})),
        ];
        let mut content = String::new();
        for (i, (event_type, body)) in events.iter().enumerate() {
            let mut map = normalize(body.as_object().cloned().unwrap_or_default(), event_type);
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, i as u64 + 1, None, "2026-10-03T10:00:00-03:00")));
            content.push('\n');
        }
        parse_log(&content)
    }

    /// O pedido da onda 1 com `changes` como o que mudou nas tarefas dela.
    fn request(log: &SpecLog, changes: BTreeMap<String, TaskChange>, lang: Locale) -> String {
        let material = Material {
            spec: "teste".into(),
            wave: 1,
            block: log.block(crate::domain::spec_events::BlockQuery::Wave(1)),
            codes: log.codes(),
            task_changes: changes,
            ..Material::default()
        };
        write(&material, lang)
    }

    #[test]
    fn a_task_whose_file_changed_after_its_text_gets_the_line_with_commits_and_files() {
        let log = plan();
        let code = log.codes().into_values().find(|code| code.contains("TASK")).expect("the task has a code");
        let change = TaskChange { commits: vec!["abc1234".into(), "def5678".into()], files: vec!["src/a.rs".into()] };
        let text = request(&log, BTreeMap::from([(code.clone(), change)]), Locale::PtBr);
        let line = "O git mudou o arquivo desta tarefa depois que o texto dela foi escrito \
                    (commits: abc1234, def5678; arquivos: `src/a.rs`). Confira no código antes de mudar. \
                    Se o que ela pede já está feito, diga na entrega que já estava feita e não mude nada.";
        assert!(text.contains(&format!("   - Arquivo: `src/a.rs`\n   - {line}\n2. ")), "{text}");
        let en = request(&log, BTreeMap::from([(code, TaskChange { commits: vec!["abc1234".into()], files: vec!["src/a.rs".into()] })]), Locale::EnUs);
        assert!(en.contains("(commits: abc1234; files: `src/a.rs`). Check the code before changing anything."), "{en}");
    }

    #[test]
    fn a_task_with_no_change_gets_no_line() {
        let log = plan();
        let text = request(&log, BTreeMap::new(), Locale::PtBr);
        assert!(!text.contains("O git mudou o arquivo desta tarefa"), "{text}");
    }
}
