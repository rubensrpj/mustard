//! A página do projeto: uma linha por spec, com a fase e o link da página
//! dela, e o caminho do índice no rodapé.
//!
//! Quem monta as linhas é quem lê o índice das specs; aqui fica só a árvore.

use super::{Document, Node, Section, Table};
use crate::platform::i18n::{translate, Locale};

/// Uma spec na página do projeto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    /// O nome da spec.
    pub spec: String,
    /// A fase em que ela está (`survey`, `plan`, …), quando se sabe.
    pub phase: Option<String>,
    /// O endereço da página publicada da spec, quando existe.
    pub url: Option<String>,
}

/// A página do projeto `project`, com as specs na ordem recebida e o caminho
/// do índice, relativo ao projeto, no rodapé.
#[must_use]
pub fn project_document(project: &str, rows: &[ProjectRow], index: &str, lang: Locale) -> Document {
    let t = |key: &str| translate(key, lang).to_string();
    let rows = rows
        .iter()
        .map(|row| {
            let phase = row
                .phase
                .as_deref()
                .map_or_else(String::new, |p| translate(&format!("page.phase.{p}"), lang).to_string());
            let page = row
                .url
                .as_deref()
                .map_or_else(|| t("page.project.none"), |url| format!("[{}]({url})", t("page.project.open")));
            vec![row.spec.clone(), phase, page]
        })
        .collect::<Vec<_>>();
    let body = if rows.is_empty() {
        vec![Node::Paragraph(t("page.empty"))]
    } else {
        vec![Node::Table(Table {
            headers: vec![t("page.project.col.spec"), t("page.project.col.phase"), t("page.project.col.page")],
            rows,
        })]
    };
    Document {
        lang: lang.as_str().to_string(),
        kind: Some(t("page.kind.project")),
        title: project.to_string(),
        meta: Vec::new(),
        body: vec![Node::Section(Section {
            anchor: Some("specs".to_string()),
            heading: t("page.project.specs"),
            collapsed: None,
            body,
        })],
        footer: Some(t("page.project.index").replace("{path}", &format!("`{index}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_spec_is_a_row_with_its_phase_and_link_and_the_index_is_in_the_footer() {
        let rows = [
            ProjectRow { spec: "trava".into(), phase: Some("running".into()), url: Some("https://x/1".into()) },
            ProjectRow { spec: "velha".into(), phase: Some("discarded".into()), url: None },
        ];
        let doc = project_document("mustard", &rows, ".claude/spec/index.ndjson", Locale::PtBr);
        let Node::Section(section) = &doc.body[0] else { panic!("{doc:?}") };
        let Node::Table(table) = &section.body[0] else { panic!("{section:?}") };
        assert_eq!(table.rows[0], ["trava", "em execução", "[abrir](https://x/1)"]);
        assert_eq!(table.rows[1], ["velha", "descartada", "sem página"]);
        assert_eq!(doc.footer.as_deref(), Some("Índice das specs: `.claude/spec/index.ndjson`"));
    }
}
