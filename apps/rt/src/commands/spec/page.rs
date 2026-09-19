//! `mustard-rt run page` — a porta das páginas do Mustard.
//!
//! `page --body <arquivo.md> --out <página.html> [--title <t>] [--subtitle
//! <s>] [--kind <rótulo>]` gera uma página avulsa (análise, relatório,
//! plano) a partir de markdown, no idioma do texto do projeto, sem opção
//! para escolher outro (`report::Render`, o mesmo layout e as fontes do
//! Google Fonts). O assistente escreve markdown, nunca HTML: HTML dentro do
//! markdown sai escapado. Sem `--title`, o título é a primeira linha
//! `# Título` do markdown, que sai do corpo.
//!
//! Saída: `{ok, path}`, com `path` exatamente como `--out` foi passado
//! (barras normais). Recusa sai com exit 1, `ok: false`, a razão em
//! `reason` e a mensagem no idioma do projeto em `hint`, e não grava nada.

use std::path::{Path, PathBuf};

use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::view::document::{Document, Meta};
use serde_json::{json, Value};

use crate::commands::spec_events::project;
use crate::report::{markdown, Render};

/// Options for `mustard-rt run page`.
pub struct PageOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// O arquivo markdown da página avulsa.
    pub body: Option<PathBuf>,
    /// Onde gravar a página avulsa; pastas ausentes são criadas.
    pub out: Option<PathBuf>,
    /// O título; sem ele, a primeira linha `# Título` do markdown.
    pub title: Option<String>,
    /// Uma linha solta sob o título, na faixa do cabeçalho.
    pub subtitle: Option<String>,
    /// O que vem depois de `Mustard · ` na faixa do cabeçalho.
    pub kind: Option<String>,
}

/// Por que uma página não foi gerada.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PageRefusal {
    MissingBody,
    UnreadableBody { path: String },
    EmptyTitle,
    WriteFailed { path: String, detail: String },
}

impl PageRefusal {
    fn reason(&self) -> &'static str {
        match self {
            Self::MissingBody => "missing-body",
            Self::UnreadableBody { .. } => "unreadable-body",
            Self::EmptyTitle => "empty-title",
            Self::WriteFailed { .. } => "write-failed",
        }
    }

    fn message(&self, lang: Locale) -> String {
        match self {
            Self::MissingBody => translate("page.missing_body", lang).to_string(),
            Self::UnreadableBody { path } => translate("page.unreadable_body", lang).replace("{path}", path),
            Self::EmptyTitle => translate("page.empty_title", lang).to_string(),
            Self::WriteFailed { path, detail } => translate("page.write_failed", lang)
                .replace("{path}", path)
                .replace("{detail}", detail),
        }
    }

    fn report(&self, lang: Locale) -> Value {
        json!({ "ok": false, "reason": self.reason(), "hint": self.message(lang) })
    }
}

/// O núcleo testável de [`run`]: o relatório da página gerada ou a recusa.
/// Toda recusa acontece antes da gravação.
pub(crate) fn build(opts: &PageOpts) -> Value {
    let project = project(&opts.root);
    let lang = project.lang;
    let (Some(body), Some(out)) = (&opts.body, &opts.out) else {
        return PageRefusal::MissingBody.report(lang);
    };
    let Ok(md) = std::fs::read_to_string(body) else {
        return PageRefusal::UnreadableBody { path: display_path(body) }.report(lang);
    };
    let (title, md) = match non_blank(opts.title.as_deref()) {
        Some(title) => (title.to_string(), md),
        None => match markdown::leading_title(&md) {
            Some(found) => found,
            None => return PageRefusal::EmptyTitle.report(lang),
        },
    };
    let doc = Document {
        lang: lang.as_str().to_string(),
        kind: non_blank(opts.kind.as_deref()).map(str::to_string),
        title,
        meta: non_blank(opts.subtitle.as_deref()).map(|s| Meta::Note(s.to_string())).into_iter().collect(),
        body: markdown::page(&md),
        footer: None,
    };
    let html = Render::Html.render(&doc);
    if let Err(e) = mustard_core::io::fs::write_atomic(out, html.as_bytes()) {
        return PageRefusal::WriteFailed { path: display_path(out), detail: e.to_string() }.report(lang);
    }
    json!({ "ok": true, "path": display_path(out) })
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// O caminho como foi pedido, com barras normais: o relatório lê igual em toda
/// plataforma.
fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// CLI entry — `mustard-rt run page`.
pub fn run(opts: &PageOpts) {
    let report = build(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string()));
    let _ = std::io::Write::flush(&mut std::io::stdout());
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn opts(root: &Path, title: Option<&str>) -> PageOpts {
        PageOpts {
            root: root.to_path_buf(),
            body: Some(root.join("corpo.md")),
            out: Some(root.join("paginas/plano.html")),
            title: title.map(str::to_string),
            subtitle: Some("onda 4".to_string()),
            kind: Some("plano".to_string()),
        }
    }

    /// O markdown sai no layout do Mustard: o título no cabeçalho, as seções
    /// com o traço do layout, a tabela na moldura e o HTML escrito à mão
    /// escapado.
    #[test]
    fn a_markdown_page_comes_out_in_the_mustard_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let body = "# Plano da onda\n\nAbertura.\n\n## Passos\n\n- ler `a.rs`\n- **testar**\n\n\
                    | A | B |\n|---|---|\n| 1 | 2 |\n\n<script>x</script>\n";
        fs::write(root.join("corpo.md"), body).unwrap();

        let report = build(&opts(root, None));
        assert_eq!(report["ok"], json!(true), "{report}");
        assert!(report["path"].as_str().unwrap().ends_with("paginas/plano.html"), "{report}");
        let html = fs::read_to_string(root.join("paginas/plano.html")).unwrap();

        assert!(html.contains("<html lang=\"pt-BR\">"), "{html}");
        assert!(html.contains("<title>Plano da onda</title>"));
        assert!(html.contains("<p class=\"brand\"><b>Mustard</b> · plano</p>"), "{html}");
        assert!(html.contains("<header class=\"top\"><p class=\"eyebrow\">plano</p><h1>Plano da onda</h1>"), "{html}");
        assert!(html.contains("<li>onda 4</li>"));
        assert!(html.contains("<p>Abertura.</p><section id=\"section-1\" class=\"block\" data-crumb=\"Passos\"><h2><span>Passos</span>"), "{html}");
        assert!(html.contains("<li>ler <code>a.rs</code></li><li><strong>testar</strong></li>"), "{html}");
        assert!(html.contains("<div class=\"table\"><table>"), "{html}");
        // O único script da página é o do motor; o do markdown sai escapado.
        assert!(html.contains("&lt;script&gt;x&lt;/script&gt;") && html.matches("<script>").count() == 1, "{html}");
        assert_eq!(html.matches("<h1>").count(), 1, "the leading title leaves the body");
        crate::report::assert_only_the_fonts_are_external(&html);
    }

    /// Depois que o motor da página inteira da spec saiu (junto com os nós
    /// que só ele usava), a página avulsa continua saindo igual: título,
    /// lista, tabela e bloco de código no mesmo HTML de antes.
    #[test]
    fn the_standalone_page_still_renders_after_the_old_engine_left() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let body = "# Plano\n\nAbertura.\n\n- um\n- dois\n\n\
                    | A | B |\n|---|---|\n| 1 | 2 |\n\n```\nlet a = 1;\n```\n";
        fs::write(root.join("corpo.md"), body).unwrap();

        let report = build(&opts(root, None));
        assert_eq!(report["ok"], json!(true), "{report}");
        let html = fs::read_to_string(root.join("paginas/plano.html")).unwrap();

        assert!(html.contains("<h1>Plano</h1>"), "{html}");
        assert!(html.contains("<p>Abertura.</p>"), "{html}");
        assert!(html.contains("<ul><li>um</li><li>dois</li></ul>"), "{html}");
        assert!(html.contains("<div class=\"table\"><table>"), "{html}");
        assert!(html.contains("<pre>let a = 1;</pre>"), "{html}");
    }

    /// Com `--title`, o markdown fica inteiro no corpo.
    #[test]
    fn a_given_title_keeps_the_markdown_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("corpo.md"), "Só um parágrafo.").unwrap();
        assert_eq!(build(&opts(root, Some("Relatório")))["ok"], json!(true));
        let html = fs::read_to_string(root.join("paginas/plano.html")).unwrap();
        assert!(html.contains("<h1>Relatório</h1>") && html.contains("<p>Só um parágrafo.</p>"), "{html}");
    }

    /// A página sai no idioma do texto do projeto: sem idioma declarado, em
    /// português do Brasil; num projeto em inglês, em inglês.
    #[test]
    fn a_page_comes_out_in_the_project_text_language() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("corpo.md"), "# Plan\n\nOne paragraph.").unwrap();
        assert_eq!(build(&opts(root, None))["ok"], json!(true));
        let html = fs::read_to_string(root.join("paginas/plano.html")).unwrap();
        assert!(html.contains("<html lang=\"pt-BR\">"), "{html}");

        fs::write(root.join("mustard.json"), r#"{"language":{"text":"en-US"}}"#).unwrap();
        assert_eq!(build(&opts(root, None))["ok"], json!(true));
        let html = fs::read_to_string(root.join("paginas/plano.html")).unwrap();
        assert!(html.contains("<html lang=\"en-US\">"), "{html}");
    }

    /// Sem título, sem corpo legível ou sem nada pedido: recusa com a razão e
    /// a mensagem, e nada gravado.
    #[test]
    fn a_page_without_title_or_readable_body_is_refused_without_writing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("corpo.md"), "Sem título.").unwrap();

        let blank = build(&opts(root, Some("   ")));
        assert_eq!(blank["reason"], json!("empty-title"));
        assert!(blank["hint"].as_str().unwrap().contains("--title"));

        let mut missing = opts(root, Some("Plano"));
        missing.body = Some(root.join("nao-existe.md"));
        let unreadable = build(&missing);
        assert_eq!(unreadable["reason"], json!("unreadable-body"));
        assert!(unreadable["hint"].as_str().unwrap().contains("nao-existe.md"));

        let mut nothing = opts(root, None);
        nothing.body = None;
        assert_eq!(build(&nothing)["reason"], json!("missing-body"));

        assert!(!root.join("paginas").exists(), "a refusal writes nothing");
    }

    /// Cada recusa tem a mensagem nos dois idiomas.
    #[test]
    fn every_refusal_speaks_both_languages() {
        for refusal in [
            PageRefusal::MissingBody,
            PageRefusal::UnreadableBody { path: "a.md".into() },
            PageRefusal::EmptyTitle,
            PageRefusal::WriteFailed { path: "a.html".into(), detail: "disco cheio".into() },
        ] {
            let (pt, en) = (refusal.message(Locale::PtBr), refusal.message(Locale::EnUs));
            assert_ne!(pt, en, "{refusal:?}");
            assert!(!pt.contains('{') && !en.contains('{'), "{pt} / {en}");
        }
    }
}
