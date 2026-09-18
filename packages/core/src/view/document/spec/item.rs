//! Um item e os campos dele: a linha recolhida (o título, a situação, a hora e
//! a marca do que mudou depois da aprovação) e os campos próprios do tipo, com
//! toda referência a outro evento escrita como o código dele.

use serde_json::Value;

use super::findings::is_plan_finding;
use super::{code_span, first_paragraph, join, one_line, Page};
use crate::domain::spec_events::{type_spec, Block, Kind, SpecEvent};
use crate::view::document::{Field, Item, Status, Tone};

/// Os registros da execução: não mudam o que foi aprovado, e não levam a
/// marca de "depois da aprovação".
const EXECUTION_RECORDS: &[&str] = &["criterion_run", "send", "delivered"];

/// Campos com o número de um evento: saem como o código dele.
const EVENT_REF: &[&str] = &["reply_to", "closes", "criterion"];

/// Campos com uma lista de números de eventos.
const EVENT_REFS: &[&str] = &["criteria", "covers", "items", "result", "targets", "contracts"];

/// Campos com números de ondas.
const WAVE_NUMBERS: &[&str] = &["wave", "waves", "depends_on"];

/// Campos cujo valor é um nome do código, um comando ou um identificador:
/// saem entre crases.
const LITERAL: &[&str] =
    &["sha", "proof", "command", "hook", "tool", "mustard", "branch", "base", "repo", "skill", "name"];

impl Page<'_> {
    /// "depois da aprovação": a marca do item do combinado, da especificação,
    /// dos critérios, das ondas ou das anotações gravado depois da aprovação
    /// que vale. Os registros da execução não a levam.
    fn after_approval(&self, event: &SpecEvent) -> Option<String> {
        let approval = self.approval?;
        let marked = matches!(
            event.block(),
            Some(Block::Agreed | Block::Specification | Block::Criteria | Block::Waves | Block::Notes)
        );
        if !marked || event.id <= approval || EXECUTION_RECORDS.contains(&event.event_type.as_str()) {
            return None;
        }
        Some(self.t("page.after_approval").to_string())
    }
    /// Um item, com a linha recolhida: o título, a situação e a hora; o que
    /// mudou depois da aprovação leva a marca.
    pub(super) fn item(&self, event: &SpecEvent, anchored: bool) -> Item {
        let text = event.str_field("text").unwrap_or_default().trim().to_string();
        let fields = self.fields(event);
        let title = if text.is_empty() {
            let origin = self.t("page.field.origin");
            let label = self.t("page.field.label");
            fields
                .iter()
                .filter(|f| f.label != origin && f.label != label)
                .map(|f| format!("{}: {}", f.label, f.value))
                .collect::<Vec<_>>()
                .join(" · ")
        } else {
            first_paragraph(&text)
        };
        Item {
            code: self.code(event.id),
            anchored,
            title,
            status: self.status(event),
            who: None,
            mark: self.after_approval(event),
            date: when(event),
            text,
            fields,
        }
    }

    /// A situação que o item mostra na linha: o resultado de um veredito, de
    /// uma execução ou de uma chamada, a situação de um ponto e se a
    /// publicação deu certo.
    fn status(&self, event: &SpecEvent) -> Option<Status> {
        let word = match event.event_type.as_str() {
            "verdict" | "criterion_run" | "call" => event.str_field("result")?,
            "point" => event.str_field("status")?,
            "publish" => {
                if event.fields.get("ok").and_then(Value::as_bool)? { "yes" } else { "no" }
            }
            _ => return None,
        };
        let tone = match word {
            "approved" | "pass" | "ok" | "closed" | "yes" => Tone::Good,
            "rejected" | "fail" | "refused" | "no" => Tone::Bad,
            "open" => Tone::Running,
            _ => Tone::Plain,
        };
        Some(Status { label: self.value_label(word), tone })
    }

    pub(super) fn field(&self, key: &str, value: String) -> Field {
        Field { label: self.t(key).to_string(), value }
    }

    /// Os campos próprios do tipo, na ordem em que o tipo os declara, e
    /// depois o rótulo do rascunho e a mensagem de origem. O texto vai no
    /// item; as palavras-chave e o campo de busca nunca aparecem.
    fn fields(&self, event: &SpecEvent) -> Vec<Field> {
        let Some(spec) = type_spec(&event.event_type) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for field in spec.fields {
            if matches!(field.name, "text" | "keys") || in_the_heading(spec.name, field.name) {
                continue;
            }
            let Some(value) = event.fields.get(field.name) else {
                continue;
            };
            let shown = self.value(field.name, field.kind, value);
            if !shown.is_empty() {
                out.push(self.field(&format!("page.field.{}", field.name), shown));
            }
        }
        // O rótulo do achado do plano já é o título da seção dele: o item não
        // o repete.
        if let Some(label) = event.str_field("label").filter(|_| !is_plan_finding(event)) {
            out.push(self.field("page.field.label", label.to_string()));
        }
        if let Some(origin) = event.int("origin") {
            out.push(self.field("page.field.origin", self.code(origin)));
        }
        out
    }

    fn value(&self, name: &str, kind: Kind, value: &Value) -> String {
        if EVENT_REF.contains(&name)
            && let Some(id) = value.as_u64()
        {
            return self.code(id);
        }
        if EVENT_REFS.contains(&name) && matches!(kind, Kind::Ints | Kind::Refs) {
            let each = |v: &Value| v.as_u64().map_or_else(|| self.plain(v), |id| self.code(id));
            return join(value.as_array().into_iter().flatten().map(each));
        }
        if WAVE_NUMBERS.contains(&name) {
            return value.as_u64().map_or_else(|| join(ints(value).into_iter().map(|n| n.to_string())), |n| n.to_string());
        }
        match kind {
            Kind::OneOf(_) => {
                let word = value.as_str().unwrap_or_default();
                return if name == "phase" { self.phase(word) } else { self.value_label(word) };
            }
            Kind::ManyOf(_) => {
                return join(value.as_array().into_iter().flatten().filter_map(Value::as_str).map(|w| self.value_label(w)));
            }
            _ => {}
        }
        match name {
            "files" => join(value.as_array().into_iter().flatten().map(|f| self.file(f))),
            "facts" => value
                .as_array()
                .into_iter()
                .flatten()
                .map(|fact| {
                    let text = one_line(fact.get("text").and_then(Value::as_str).unwrap_or_default());
                    match fact.get("source").and_then(Value::as_str) {
                        Some(source) => format!("{text} ({})", code_span(source)),
                        None => text,
                    }
                })
                .collect::<Vec<_>>()
                .join("; "),
            "examples" => join(value.as_array().into_iter().flatten().map(|ex| {
                let path = ex.get("path").and_then(Value::as_str).unwrap_or_default();
                let why = one_line(ex.get("why").and_then(Value::as_str).unwrap_or_default());
                format!("{}: {why}", code_span(path))
            })),
            "skills" => join(value.as_array().into_iter().flatten().map(|s| {
                let name = s.get("name").and_then(Value::as_str).unwrap_or_default();
                let sha = s.get("sha").and_then(Value::as_str).unwrap_or_default();
                format!("{} ({})", code_span(name), code_span(sha))
            })),
            "criteria" => join(value.as_array().into_iter().flatten().map(|c| {
                let id = c.get("criterion").and_then(Value::as_u64).map_or_else(String::new, |id| self.code(id));
                let key = if c.get("tests_rule").and_then(Value::as_bool) == Some(true) {
                    "page.value.tests_rule"
                } else {
                    "page.value.not_tests_rule"
                };
                format!("{id} ({})", self.t(key))
            })),
            "lessons" if kind == Kind::Objects => join(value.as_array().into_iter().flatten().map(|l| {
                let id = l.get("lesson").map(|v| self.plain(v)).unwrap_or_default();
                let key = if l.get("repeated").and_then(Value::as_bool) == Some(true) {
                    "page.value.repeated"
                } else {
                    "page.value.not_repeated"
                };
                format!("{id} ({})", self.t(key))
            })),
            "witness" => {
                let question = value.get("question").and_then(Value::as_str).unwrap_or_default();
                let answer = value.get("answer").and_then(Value::as_str).unwrap_or_default();
                format!("{} → {}", one_line(question), one_line(answer))
            }
            "filter" => {
                let get = |k: &str| value.get(k).and_then(Value::as_str).unwrap_or_default();
                format!("{} {} → {}", self.t(&format!("page.type.{}", get("type"))), get("from"), get("to"))
            }
            "url" => value.as_str().map_or_else(String::new, |url| format!("[{url}]({url})")),
            // Um valor que já traz as próprias crases é markdown escrito por
            // quem gravou, como uma prova que cita o comando no meio da
            // frase: sai como está, senão as crases de dentro apareceriam na
            // página. Sem crase nenhuma, o valor inteiro é o código.
            _ if LITERAL.contains(&name) => value.as_str().map_or_else(
                || self.plain(value),
                |text| if text.contains('`') { one_line(text) } else { code_span(text) },
            ),
            _ => self.plain(value),
        }
    }

    /// Um arquivo de uma tarefa (`{"path", "new"}`) ou de um entregou (o
    /// caminho puro).
    fn file(&self, file: &Value) -> String {
        let path = file.as_str().or_else(|| file.get("path").and_then(Value::as_str)).unwrap_or_default();
        let new = file.get("new").and_then(Value::as_bool) == Some(true);
        if new {
            format!("{} ({})", code_span(path), self.t("page.value.new"))
        } else {
            code_span(path)
        }
    }

    /// Qualquer valor em uma linha: lista separada por vírgula, objeto como
    /// pares `chave: valor`.
    fn plain(&self, value: &Value) -> String {
        match value {
            Value::Null => String::new(),
            Value::Bool(b) => self.t(if *b { "page.value.yes" } else { "page.value.no" }).to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => one_line(s),
            Value::Array(items) => join(items.iter().map(|v| self.plain(v))),
            Value::Object(map) => map
                .iter()
                .map(|(k, v)| format!("{k}: {}", self.plain(v)))
                .collect::<Vec<_>>()
                .join("; "),
        }
    }

    pub(super) fn phase(&self, phase: &str) -> String {
        self.t(&format!("page.phase.{phase}")).to_string()
    }

    pub(super) fn value_label(&self, word: &str) -> String {
        self.t(&format!("page.value.{word}")).to_string()
    }
}

/// O número da onda já está no subtítulo da parte dela: a onda não repete o
/// próprio número, e a tarefa, o envio e o entregou não repetem a onda.
fn in_the_heading(event_type: &str, field: &str) -> bool {
    matches!((event_type, field), ("wave", "n") | ("task" | "send" | "delivered", "wave"))
}

/// A hora que o evento gravou, até o minuto: "2026-09-11 21:03".
fn when(event: &SpecEvent) -> Option<String> {
    let at = event.at();
    (!at.is_empty()).then(|| at.get(..16).unwrap_or(at).replace('T', " "))
}

fn ints(value: &Value) -> Vec<u64> {
    value.as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::tests::*;
    use crate::domain::spec_events::parse_log;
    use crate::platform::i18n::Locale;
    use crate::view::document::{spec_document, Item, Node, Tone, WavePrompts};

    /// Cada item traz a linha recolhida: o título (o primeiro parágrafo do
    /// texto, ou os campos quando não há texto), a situação com o tom dela e
    /// a hora que o evento gravou.
    #[test]
    fn each_item_row_has_its_title_status_and_date() {
        let content = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(2, "decision", ",\"text\":\"**Primeira linha.**\\n\\nO resto do texto.\",\"keys\":[\"k\"],\"why\":\"w\",\"origin\":1"),
            line(3, "verdict", ",\"author\":\"review\",\"wave\":1,\"result\":\"rejected\",\"text\":\"Faltou o teste.\",\"criteria\":[]"),
            line(4, "point", ",\"block\":\"limits\",\"gap\":\"Tamanho\",\"from\":\"gap\",\"status\":\"open\",\"facts\":[],\"origin\":1"),
            line(5, "publish", ",\"page\":\"spec\",\"milestone\":\"approval\",\"ok\":true,\"url\":\"https://claude.ai/code/artifact/x\""),
            line(6, "criterion", ",\"when\":\"w\",\"then\":\"t\",\"proof\":\"p\",\"origin\":1"),
            line(7, "criterion_run", ",\"author\":\"binary\",\"criterion\":6,\"result\":\"pass\",\"exit\":0,\"ms\":5"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let all: Vec<&Item> = sections(&doc).into_iter().flat_map(items).collect();
        let row = |code: &str| all.iter().find(|i| i.code == code).copied().unwrap_or_else(|| panic!("{code}"));
        let status = |code: &str| row(code).status.clone().map(|s| (s.label, s.tone));

        assert_eq!(row("MSTD-DEC-0001").title, "**Primeira linha.**");
        assert_eq!(row("MSTD-DEC-0001").date.as_deref(), Some("2026-09-12 10:02"));
        assert_eq!(status("MSTD-DEC-0001"), None);
        assert_eq!(status("MSTD-VERD-0001"), Some(("reprovada".into(), Tone::Bad)));
        assert_eq!(status("MSTD-POINT-0001"), Some(("pendente".into(), Tone::Running)));
        assert_eq!(status("MSTD-PUB-0001"), Some(("sim".into(), Tone::Good)));
        assert_eq!(status("MSTD-CRUN-0001"), Some(("passou".into(), Tone::Good)));
        let publish = row("MSTD-PUB-0001");
        assert!(publish.text.is_empty());
        assert!(publish.title.starts_with("Página: spec · Marco: aprovação · Deu certo: sim"), "{}", publish.title);
        let criterion = row("MSTD-CRIT-0001");
        assert!(!criterion.title.contains("Origem"), "the origin stays out of the title: {}", criterion.title);
        assert_eq!(criterion.note(), None, "a row with only its time says nothing more in the .md");
    }

    /// Toda referência a outro evento sai como o código dele, e o número da
    /// onda vira o título do grupo dela.
    #[test]
    fn references_come_out_as_codes_and_waves_get_their_heading() {
        let content = [
            line(1, "criterion", ",\"when\":\"w\",\"then\":\"t\",\"proof\":\"cargo test\",\"origin\":1"),
            line(2, "wave", ",\"n\":1,\"text\":\"Objetivo.\",\"criteria\":[1],\"done_when\":\"d\",\"origin\":1"),
            line(3, "task", ",\"wave\":1,\"text\":\"Tarefa.\",\"files\":[{\"path\":\"a.rs\",\"new\":true}],\"origin\":1"),
            line(4, "criterion_run", ",\"criterion\":1,\"result\":\"pass\",\"exit\":0,\"ms\":5"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let waves = section(&doc, "waves");
        let one = group(waves, "waves-1");
        assert_eq!((one.title.as_str(), one.summary.as_str()), ("Onda 1", "Objetivo."));
        let wave = items(waves)[0];
        let criteria = wave.fields.iter().find(|f| f.label == "Critérios").expect("criteria field");
        assert_eq!(criteria.value, "MSTD-CRIT-0001");
        let state = wave.fields.iter().find(|f| f.label == "Estado da onda").expect("wave state");
        assert_eq!(state.value, "a fazer");
        assert!(wave.fields.iter().all(|f| f.label != "Número"), "the number is in the heading");
        let task = items(waves)[1];
        assert_eq!(task.fields[0].value, "`a.rs` (novo)");
        let criterion = items(section(&doc, "criteria"))[0];
        assert!(criterion.fields.iter().any(|f| f.value == "`cargo test`"));
        assert!(criterion.fields.iter().any(|f| f.value == "passou (MSTD-CRUN-0001)"), "{criterion:?}");
    }

    /// O ponto do levantamento que outro fechou mostra o código do ponto que
    /// o fechou, que é um endereço da página e sai como link; também quando
    /// o fechamento aponta a versão antiga de um ponto revisto. O ponto ainda
    /// aberto e o próprio fechamento não mostram fechamento nenhum.
    #[test]
    fn a_closed_survey_point_links_to_the_point_that_closed_it() {
        let point = |status: &str, gap: &str, more: &str| {
            format!(",\"block\":\"limits\",\"gap\":\"{gap}\",\"from\":\"gap\",\"status\":\"{status}\",\"facts\":[],\"origin\":1{more}")
        };
        let content = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(2, "point", &point("open", "Tamanho", "")),
            line(3, "point", &point("open", "Prazo", "")),
            line(4, "point", &point("closed", "Tamanho", ",\"closes\":2")),
            line(5, "point", &point("open", "Formato", "")),
            line(6, "point", &point("open", "Formato revisto", ",\"replaces\":5")),
            line(7, "point", &point("not_applicable", "Formato", ",\"closes\":5,\"reason\":\"não se aplica\"")),
        ]
        .concat();
        let closed_by = |lang: Locale, label: &str| -> Vec<(String, Option<String>)> {
            let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), lang);
            Node::items(&group(section(&doc, "agreed"), "agreed-point").body)
                .into_iter()
                .map(|item| {
                    let link = item.fields.iter().find(|f| f.label == label).map(|f| f.value.clone());
                    if let Some(code) = &link {
                        assert!(doc.anchors().contains(code), "{code} is an address on the page, so it comes out as a link");
                    }
                    (item.code.clone(), link)
                })
                .collect()
        };
        let expected = |code: &str, link: Option<&str>| (code.to_string(), link.map(str::to_string));
        assert_eq!(
            closed_by(Locale::PtBr, "Fechado por"),
            [
                expected("MSTD-POINT-0001", Some("MSTD-POINT-0003")),
                expected("MSTD-POINT-0002", None),
                expected("MSTD-POINT-0003", None),
                expected("MSTD-POINT-0004", Some("MSTD-POINT-0005")),
                expected("MSTD-POINT-0005", None),
            ]
        );
        assert_eq!(closed_by(Locale::EnUs, "Closed by")[0], expected("MSTD-POINT-0001", Some("MSTD-POINT-0003")));
    }

    /// Uma tarefa sem arquivo sai sem a linha dos arquivos; a que cita
    /// arquivo continua mostrando a linha.
    #[test]
    fn a_task_without_files_shows_no_files_line() {
        let content = [
            line(1, "wave", ",\"n\":1,\"text\":\"Objetivo.\",\"criteria\":[1],\"done_when\":\"d\",\"origin\":1"),
            line(2, "task", ",\"wave\":1,\"text\":\"Medir de novo.\",\"origin\":1"),
            line(3, "task", ",\"wave\":1,\"text\":\"Mudar o leitor.\",\"files\":[{\"path\":\"a.rs\"}],\"origin\":1"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let tasks: Vec<&Item> =
            items(section(&doc, "waves")).into_iter().filter(|i| i.code.starts_with("MSTD-TASK-")).collect();
        let files = |item: &Item| item.fields.iter().any(|f| f.label == "Arquivos");
        assert_eq!((files(tasks[0]), files(tasks[1])), (false, true), "{tasks:?}");
    }

    /// Uma prova que já traz o comando entre crases no meio da frase sai
    /// como foi escrita, sem crase a mais em volta; uma prova sem crase
    /// nenhuma sai inteira como código.
    #[test]
    fn a_proof_with_its_own_code_marks_comes_out_as_written() {
        let content = [
            line(1, "criterion", ",\"when\":\"w\",\"then\":\"t\",\"proof\":\"`find . -type f | wc -l` = 3\",\"origin\":1"),
            line(2, "criterion", ",\"when\":\"w\",\"then\":\"t\",\"proof\":\"cargo test\",\"origin\":1"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let proofs: Vec<&str> = items(section(&doc, "criteria"))
            .iter()
            .flat_map(|i| i.fields.iter())
            .filter(|f| f.label == "Prova")
            .map(|f| f.value.as_str())
            .collect();
        assert_eq!(proofs, ["`find . -type f | wc -l` = 3", "`cargo test`"]);
    }

    /// O que a spec ganhou depois da aprovação que vale sai marcado, com a
    /// hora do próprio item: a regra e a onda novas e o pedido. O que veio
    /// antes, os registros da execução e a conversa não levam a marca, e uma
    /// spec nunca aprovada não marca nada.
    #[test]
    fn the_page_marks_what_changed_after_the_approval_with_its_time() {
        let before_approval = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(2, "rule", ",\"text\":\"Regra antiga.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":1"),
            line(3, "wave", ",\"n\":1,\"text\":\"Onda um.\",\"criteria\":[2],\"done_when\":\"d\",\"origin\":1"),
            line(4, "state", ",\"author\":\"binary\",\"phase\":\"plan\""),
        ]
        .concat();
        let after_approval = [
            line(5, "state", ",\"author\":\"user\",\"phase\":\"approved\",\"witness\":{\"question\":\"Aprovar?\",\"answer\":\"Aprovar\"}"),
            line(6, "rule", ",\"text\":\"Regra nova.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":1"),
            line(7, "wave", ",\"n\":2,\"text\":\"Onda dois.\",\"criteria\":[2],\"done_when\":\"d\",\"origin\":1"),
            line(8, "send", ",\"author\":\"binary\",\"wave\":2,\"role\":\"wave\",\"lines\":1,\"chars\":1,\"items\":[7],\"mustard\":\"0.2.0\""),
            line(9, "request", ",\"text\":\"Mais uma onda.\",\"keys\":[\"k\"],\"effect\":\"new_waves\",\"origin\":1"),
        ]
        .concat();
        let notes = |content: &str, lang: Locale| -> Vec<(String, Option<String>)> {
            let doc = spec_document("s", &parse_log(content), &WavePrompts::new(), lang);
            sections(&doc)
                .into_iter()
                .filter(|s| s.anchor.as_deref() != Some("conversation"))
                .flat_map(|s| items(s).into_iter().map(|i| (i.code.clone(), i.note())).collect::<Vec<_>>())
                .collect()
        };
        let marked = |got: &[(String, Option<String>)]| -> Vec<(String, String)> {
            got.iter().filter_map(|(code, note)| note.clone().map(|n| (code.clone(), n))).collect()
        };

        let approved = format!("{before_approval}{after_approval}");
        assert_eq!(
            marked(&notes(&approved, Locale::PtBr)),
            [
                ("MSTD-RULE-0002".to_string(), "depois da aprovação · 2026-09-12 10:06".to_string()),
                ("MSTD-WAVE-0002".to_string(), "depois da aprovação · 2026-09-12 10:07".to_string()),
                ("MSTD-REQ-0001".to_string(), "depois da aprovação · 2026-09-12 10:09".to_string()),
            ]
        );
        let english = marked(&notes(&approved, Locale::EnUs));
        assert_eq!(english[0].1, "after the approval · 2026-09-12 10:06");

        let doc = spec_document("s", &parse_log(&approved), &WavePrompts::new(), Locale::PtBr);
        let talk = items(section(&doc, "conversation"));
        assert_eq!(talk[0].note().as_deref(), Some("mensagem · usuário · 2026-09-12 10:01"), "the conversation keeps its note");
        assert_eq!(talk[0].who.as_deref(), Some("mensagem · usuário"), "the row says what and whose it is");

        let never = before_approval.replace("\"phase\":\"plan\"", "\"phase\":\"survey\"") + &after_approval.replace("approved", "plan");
        assert!(marked(&notes(&never, Locale::PtBr)).is_empty(), "a spec never approved marks nothing");
    }

    /// Numa aprovação revista (a spec que nasceu aprovada e ganhou a branch
    /// depois), a marca parte da primeira versão da aprovação: a regra
    /// gravada entre ela e a revisão continua marcada.
    #[test]
    fn a_revised_approval_keeps_marking_from_its_first_version() {
        let witness = ",\"witness\":{\"question\":\"Aprovar?\",\"answer\":\"Aprovar\"}";
        let content = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(2, "state", &format!(",\"author\":\"user\",\"phase\":\"approved\"{witness}")),
            line(3, "rule", ",\"text\":\"Entre as duas.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":1"),
            line(4, "state", &format!(",\"author\":\"binary\",\"phase\":\"approved\",\"branch\":\"feature/x\",\"replaces\":2{witness}")),
            line(5, "rule", ",\"text\":\"Depois da revisão.\",\"keys\":[\"k\"],\"example\":\"e\",\"origin\":1"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let rules: Vec<(String, Option<String>)> =
            items(section(&doc, "agreed")).iter().map(|i| (i.text.clone(), i.note())).collect();
        let rule = |text: &str, note: &str| (text.to_string(), Some(note.to_string()));
        assert_eq!(
            rules,
            [
                rule("Entre as duas.", "depois da aprovação · 2026-09-12 10:03"),
                rule("Depois da revisão.", "depois da aprovação · 2026-09-12 10:05"),
            ]
        );
    }
}
