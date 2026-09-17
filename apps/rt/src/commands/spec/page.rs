//! `mustard-rt run page` — a porta das páginas do Mustard.
//!
//! Duas formas, o mesmo motor (`report::Render`), o mesmo layout e as fontes
//! do Google Fonts:
//!
//! - `page --body <arquivo.md> --out <página.html> [--title <t>]
//!   [--subtitle <s>] [--kind <rótulo>]` gera uma página avulsa (análise,
//!   relatório, plano) a partir de markdown, no idioma do texto do projeto,
//!   sem opção para escolher outro. O assistente
//!   escreve markdown, nunca HTML: HTML dentro do markdown sai escapado. Sem
//!   `--title`, o título é a primeira linha `# Título` do markdown, que sai
//!   do corpo.
//! - `page --spec <nome>` refaz o `spec.md` e o `spec.html` da spec a partir
//!   do `spec.ndjson`, e a página do projeto a partir do índice, sem gravar
//!   nada no arquivo de eventos.
//!
//! Saída: `{ok, path}` na página avulsa, com `path` exatamente como `--out`
//! foi passado (barras normais), e `{ok, spec, md, html, project}` na da spec,
//! com os caminhos relativos ao projeto. A conferência antes de publicar
//! acrescenta `withheld` (os itens que ainda guardam no arquivo um trecho com
//! cara de segredo, que saiu da página como "…"), `trimmed` (quantos registros da conversa ficaram só
//! no `.md`) e `warnings`, quando há o que dizer. Recusa sai com exit 1, `ok: false`, a razão
//! em `reason` e a mensagem no idioma do projeto em `hint`, e não grava nada.

use std::path::{Path, PathBuf};

use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::view::document::{Document, Meta};
use serde_json::{json, Value};

use crate::commands::spec_events::{pages, project, refused};
use crate::report::{markdown, Render};

/// Options for `mustard-rt run page`.
pub struct PageOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec cuja página e cujo `.md` são refeitos.
    pub spec: Option<String>,
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

/// Por que uma página avulsa não foi gerada.
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
    if let Some(spec) = opts.spec.as_deref() {
        return match pages::refresh(&project.root, spec, lang) {
            Ok(written) => {
                let mut report = json!({ "ok": true, "spec": spec.trim(), "md": written.md, "html": written.html });
                if let Some(page) = written.project {
                    report["project"] = json!(page);
                }
                if !written.withheld.is_empty() {
                    report["withheld"] = json!(written.withheld);
                }
                if written.trimmed > 0 {
                    report["trimmed"] = json!(written.trimmed);
                }
                if !written.warnings.is_empty() {
                    report["warnings"] = json!(written.warnings);
                }
                report
            }
            Err(refusal) => refused(&refusal, lang),
        };
    }
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
            spec: None,
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
        assert!(html.contains("<header class=\"doc\"><p class=\"kind\">Mustard · plano</p><h1>Plano da onda</h1>"));
        assert!(html.contains("<li>onda 4</li>"));
        assert!(html.contains("<p>Abertura.</p><section><h2>Passos</h2>"), "{html}");
        assert!(html.contains("<li>ler <code>a.rs</code></li><li><strong>testar</strong></li>"), "{html}");
        assert!(html.contains("<div class=\"table\"><table>"), "{html}");
        assert!(html.contains("&lt;script&gt;x&lt;/script&gt;") && !html.contains("<script>"), "{html}");
        assert_eq!(html.matches("<h1>").count(), 1, "the leading title leaves the body");
        crate::report::assert_only_the_fonts_are_external(&html);
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

    /// Pelo `page --spec`, numa spec de teste: cada forma comum de escrever
    /// um segredo sai da página como "…", com o resto da mensagem legível e o
    /// código dela em `withheld`, e o `.md` local continua inteiro; o que tem letra e número
    /// sem ser segredo — código de item, data, caminho com linha, leitura de
    /// variável de ambiente — continua na página.
    #[test]
    fn page_spec_withholds_every_common_secret_form_and_keeps_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let events = root.join(".claude/spec/segredo/spec.ndjson");
        let put = |event_type: &str, draft: Value| {
            mustard_core::io::spec_events::write(&events, event_type, draft.as_object().cloned().unwrap(), &[])
                .unwrap()
                .code
                .unwrap_or_default()
        };
        put("state", json!({"phase": "survey"}));
        let npm = format!("npm_{}", "a1B2c3".repeat(6));
        let project_key = format!("sk-proj-{}", "Ab_3-".repeat(8));
        let secrets = [
            "DB_PASSWORD=S3nh4F0rte2024",
            "GITHUB_TOKEN=a1b2c3d4e5f6g7h8i9j0",
            "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            r#"{"password": "Hunt3rDois"}"#,
            "client_secret=9f8e7d6c5b4a3210",
            "postgres://loja:Banc0Loja@db.interno:5432/loja",
            "Authorization: Bearer 8f14e45fceea167a5a36dedd4bea2543",
            &project_key,
            &npm,
            "a senha do banco: Pr0dSenha",
            "SECRET_KEY=Ch4veDoSite",
            r#""Jwt": {"Key": "Ch4veDoJwt"}"#,
            "AccountKey=Ch4veDoAzure==;EndpointSuffix=core.windows.net",
            "a chave é Ch4veDaFrase",
            "postgres://app:senhadobanco@db",
            "redis://:senhadocache@cache",
        ];
        let mut codes = Vec::new();
        for secret in secrets {
            codes.push(put("message", json!({"author": "user", "text": format!("o valor é {secret} e pronto")})));
        }
        let ordinary = [
            "token: MSTD-TASK-0101",
            "token: 2026-09-17T02:35:53-03:00",
            "secret: apps/rt/src/shared/rtk_gain.rs:120",
            "token: process.env.GITHUB_TOKEN2",
            "chave: MSTD-DEC-0138",
            "SECRET_KEY=process.env.SECRET_KEY2",
            "a forma é postgres://usuário:senha@host",
            "redis://:${REDIS_PASSWORD}@cache",
        ];
        for text in ordinary {
            put("message", json!({"author": "user", "text": text}));
        }

        let report = build(&PageOpts {
            root: root.to_path_buf(),
            spec: Some("segredo".into()),
            body: None,
            out: None,
            title: None,
            subtitle: None,
            kind: None,
        });
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["withheld"], json!(codes), "{report}");
        let html = fs::read_to_string(root.join(".claude/spec/segredo/spec.html")).unwrap();
        let md = fs::read_to_string(root.join(".claude/spec/segredo/spec.md")).unwrap();
        for value in ["S3nh4F0rte2024", "a1b2c3d4e5f6g7h8i9j0", "bPxRfiCYEXAMPLEKEY", "Hunt3rDois", "9f8e7d6c5b4a3210",
            "Banc0Loja", "8f14e45fceea167a5a36dedd4bea2543", &project_key, &npm, "Pr0dSenha", "Ch4veDoSite",
            "Ch4veDoJwt", "Ch4veDoAzure", "Ch4veDaFrase", "senhadobanco", "senhadocache"]
        {
            assert!(!html.contains(value), "{value} reached the page");
            assert!(md.contains(value), "{value} left the local .md");
        }
        for text in ["MSTD-TASK-0101", "2026-09-17T02:35:53-03:00", "rtk_gain.rs:120", "process.env.GITHUB_TOKEN2",
            "MSTD-DEC-0138", "process.env.SECRET_KEY2", "postgres://usuário:senha@host", "REDIS_PASSWORD"]
        {
            assert!(html.contains(text), "{text} was withheld without being a secret");
        }
        assert!(html.contains("o valor é DB_PASSWORD=… e pronto"), "only the excerpt leaves the page");
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
