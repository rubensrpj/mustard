//! `mustard-rt run spec-doc --spec <slug>` — monta o resumo legível da spec em
//! `.claude/spec/<slug>/resumo.html`, no layout padrão do Mustard.
//!
//! ## Por que existe
//!
//! A spec, as ondas, as skills de cada onda, o material da conversa e a prova
//! dos critérios moram em arquivos separados (`spec.md`, `wave-N-*/spec.md`,
//! `spec-material.json`, `ac-proof.json`), e o terminal não junta nada disso num
//! texto que dê para ler. Em 10/09/2026 o usuário recusou a aprovação de uma
//! spec por isso: "não consigo ler a spec por isso preciso do html". Esta página
//! junta tudo, e vem ANTES de qualquer pergunta de aprovação.
//!
//! ## O que a página traz, nesta ordem
//!
//! Cabeçalho (título, spec, branch, base, estágio, data), resumo da conversa,
//! onde estamos (os sete passos), o que foi esclarecido, decisões, riscos,
//! antes e depois (o `flow` do material), a spec, critérios com o estado da
//! prova, cada onda com tarefas, arquivos, critérios, obrigações externas e
//! skills, evidências, pendências abertas e o próximo passo. Cada seção só
//! aparece quando tem conteúdo; o texto sai do catálogo `i18n` no idioma da
//! spec.
//!
//! ## Contrato
//!
//! Saída: `{ok, path, url, hash, changed, publishedUrl}`. `changed` diz se o
//! conteúdo mudou
//! desde a última geração — o arquivo só é regravado quando muda, então rodar de
//! novo não custa nada. Exit 0 quando monta; 1 numa recusa (nome inválido, spec
//! inexistente, disco sem escrita). Fail-open no conteúdo: um arquivo ausente ou
//! ilegível só apaga a seção que dependia dele.
//!
//! ## O endereço publicado
//!
//! Quem publica a página no claude.ai é o assistente, então o binário nunca
//! fica sabendo o endereço sozinho. `--published-url <url>` o grava em
//! `.claude/spec/<slug>/published-url`, uma linha, antes de montar a página; o
//! relatório o devolve em `publishedUrl`, e [`published_url`] é o leitor único
//! de quem precisa dele. O endereço fica fora da página: gravá-lo não muda o
//! `hash`, e o gancho de fim de resposta não pede outra publicação por isso.
//!
//! ## A exceção à saída byte-estável, de propósito
//!
//! Os guards do `run` pedem saída sem caminho de máquina, e `path` segue a regra
//! (relativo ao repositório). `url` é a exceção deliberada: é o `file://`
//! ABSOLUTO que o usuário clica, e um link relativo não abre nada. `changed`
//! também depende do disco — é justamente a pergunta que ele responde.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use mustard_core::platform::i18n::{wave_label, I18n, Locale};
use serde::{Deserialize, Serialize};

use crate::commands::agent::render::prompt_ref::fnv1a64;
use crate::commands::agent::render::skills::{arquivos_paths, wave_molds, MoldCover};
use crate::commands::event::pending::open_pending;
use crate::commands::event::work_branch::slug_of_work_branch;
use crate::commands::review::qa_run::{extract_ac_section, parse_ac_items};
use crate::commands::spec::material_add::{read_material, Material, Severity};
use crate::commands::spec::spec_sections::section_block;
use crate::report::{escape, Report};

/// O arquivo que o comando escreve, dentro do diretório da spec.
pub(crate) const DOC_FILE: &str = "resumo.html";

/// O arquivo, dentro do diretório da spec, com o endereço em que o assistente
/// publicou a página — uma linha, gravada por `spec-doc --published-url`.
pub(crate) const PUBLISHED_URL_FILE: &str = "published-url";

/// Os sete passos do processo, na ordem em que acontecem — cada um é a raiz
/// das chaves `doc.step.<passo>.name` / `.desc` do catálogo.
const STEPS: [&str; 7] = ["analyze", "plan", "approval", "execute", "review", "verify", "close"];

/// Options for `mustard-rt run spec-doc`.
pub struct SpecDocOpts {
    /// O slug da spec em `.claude/spec/`.
    pub spec: String,
    /// O endereço em que o assistente publicou a página no claude.ai; gravado
    /// em [`PUBLISHED_URL_FILE`] antes de a página ser montada.
    pub published_url: Option<String>,
}

/// O relatório JSON. `path` é relativo ao repositório; `url` é o `file://`
/// absoluto que o usuário clica.
#[derive(Debug, Serialize)]
pub(crate) struct SpecDocReport {
    pub(crate) ok: bool,
    pub(crate) path: String,
    pub(crate) url: String,
    /// FNV-1a 64 do conteúdo, em hexadecimal — muda só quando a página muda.
    pub(crate) hash: String,
    /// `true` quando o conteúdo mudou desde a última geração (e foi regravado).
    pub(crate) changed: bool,
    /// O endereço publicado gravado para a unidade; `null` quando não há.
    #[serde(rename = "publishedUrl")]
    pub(crate) published_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remedy: Option<String>,
}

/// Uma recusa: o código do erro e o que fazer.
type Refusal = (&'static str, &'static str);

const INVALID_SPEC: Refusal = (
    "invalid_spec",
    "pass the slug of a spec under .claude/spec/ — no path separators, no `..`",
);
const UNKNOWN_SPEC: Refusal = (
    "unknown_spec",
    "no spec directory of that name — build the page for the unit that is open",
);

impl SpecDocReport {
    fn refused((error, remedy): Refusal) -> Self {
        Self {
            ok: false,
            path: String::new(),
            url: String::new(),
            hash: String::new(),
            changed: false,
            published_url: None,
            error: Some(error.to_string()),
            remedy: Some(remedy.to_string()),
        }
    }
}

/// Monta a página da spec `spec` sob `root` e a grava quando o conteúdo mudou.
#[must_use]
pub(crate) fn generate(root: &Path, spec: &str) -> SpecDocReport {
    let Ok(paths) =
        mustard_core::ClaudePaths::for_project(root).and_then(|p| p.for_spec(spec))
    else {
        return SpecDocReport::refused(INVALID_SPEC);
    };
    let dir = paths.dir().to_path_buf();
    if !dir.is_dir() {
        return SpecDocReport::refused(UNKNOWN_SPEC);
    }
    let html = render(root, spec, &dir);
    let file = dir.join(DOC_FILE);
    let changed = std::fs::read_to_string(&file).ok().as_deref() != Some(html.as_str());
    if changed && mustard_core::io::fs::write_atomic(&file, html.as_bytes()).is_err() {
        return SpecDocReport::refused((
            "write_failed",
            "the page could not be written — check the spec directory is writable",
        ));
    }
    SpecDocReport {
        ok: true,
        path: repo_relative(root, &file),
        url: file_url(&file),
        hash: format!("{:016x}", fnv1a64(&[&html])),
        changed,
        // O endereço fica fora da página: gravá-lo nunca muda o `hash`, senão o
        // gancho de fim de resposta pediria outra publicação a cada gravação.
        published_url: read_published(&dir),
        error: None,
        remedy: None,
    }
}

/// O endereço em que a página da unidade `slug` foi publicada, lido de
/// `.claude/spec/<slug>/published-url`. `None` sem arquivo, com ele vazio ou
/// com um nome que não é de spec. O leitor único de quem precisa do endereço:
/// a retomada, a barra de status e o gancho de fim de resposta.
#[must_use]
pub fn published_url(root: &Path, slug: &str) -> Option<String> {
    let paths =
        mustard_core::ClaudePaths::for_project(root).and_then(|p| p.for_spec(slug)).ok()?;
    read_published(paths.dir())
}

/// A primeira linha não vazia de [`PUBLISHED_URL_FILE`] em `dir`.
fn read_published(dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(dir.join(PUBLISHED_URL_FILE)).ok()?;
    raw.lines().map(str::trim).find(|line| !line.is_empty()).map(str::to_string)
}

/// Grava `url` como o endereço publicado da unidade `slug`, numa linha. Recusa
/// o que não é um link `http(s)://` inteiro e sem espaço — o arquivo guarda um
/// endereço que alguém vai abrir — e uma spec que não existe. Recusa também
/// caractere de controle: um ESC gravado vira sequência de terminal na barra de
/// status e na retomada, que imprimem o endereço cru.
fn record_published_url(root: &Path, slug: &str, url: &str) -> Result<(), Refusal> {
    let url = url.trim();
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    if rest.is_none_or(str::is_empty)
        || url.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err((
            "invalid_published_url",
            "pass the address the page was published at — one http(s):// link, no spaces",
        ));
    }
    let Ok(paths) =
        mustard_core::ClaudePaths::for_project(root).and_then(|p| p.for_spec(slug))
    else {
        return Err(INVALID_SPEC);
    };
    let dir = paths.dir();
    if !dir.is_dir() {
        return Err(UNKNOWN_SPEC);
    }
    mustard_core::io::fs::write_atomic(dir.join(PUBLISHED_URL_FILE), format!("{url}\n").as_bytes())
        .map_err(|_| {
            (
                "write_failed",
                "the address could not be written — check the spec directory is writable",
            )
        })
}

/// CLI entry — `mustard-rt run spec-doc`.
pub fn run(opts: &SpecDocOpts) {
    let root = PathBuf::from(crate::shared::context::project_dir());
    // O endereço é gravado antes de a página ser montada: o relatório já sai
    // com ele, e uma recusa não deixa nada gravado.
    let recorded = opts
        .published_url
        .as_deref()
        .map_or(Ok(()), |url| record_published_url(&root, &opts.spec, url));
    let report = match recorded {
        Ok(()) => generate(&root, &opts.spec),
        Err(refusal) => SpecDocReport::refused(refusal),
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    std::process::exit(i32::from(!report.ok));
}

// ---------------------------------------------------------------------------
// O que a página lê
// ---------------------------------------------------------------------------

/// A fatia do `meta.json` que a página usa. Leitura tolerante: chave ausente,
/// nula ou desconhecida não derruba nada, só deixa o campo vazio.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    #[serde(default)]
    stage: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default)]
    checkpoint: Option<String>,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    completed_waves: Option<Vec<u32>>,
}

fn read_meta(dir: &Path) -> Meta {
    std::fs::read_to_string(dir.join("meta.json"))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Uma onda como a página a mostra, lida do `wave-N-*/spec.md`.
struct WaveDoc {
    number: u32,
    summary: String,
    /// O corpo cru da seção de tarefas (markdown).
    tasks: String,
    files: Vec<String>,
    /// O corpo cru das obrigações de realidade (markdown).
    obligations: String,
    /// Os critérios que a onda satisfaz, do `satisfies:` do frontmatter.
    satisfies: Vec<String>,
    done: bool,
}

fn read_waves(dir: &Path, meta: &Meta) -> Vec<WaveDoc> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let completed = meta.completed_waves.clone().unwrap_or_default();
    let mut waves = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(number) = wave_number(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        let text = std::fs::read_to_string(path.join("spec.md")).unwrap_or_default();
        let done = completed.contains(&number)
            || read_meta(&path).outcome.as_deref().is_some_and(|o| o.eq_ignore_ascii_case("completed"));
        waves.push(WaveDoc {
            number,
            summary: one_line(first_paragraph(&section_body(&text, "summary"))),
            tasks: section_body(&text, "tasks"),
            files: arquivos_paths(&text),
            obligations: section_body(&text, "reality-obligations"),
            satisfies: satisfies(&text),
            done,
        });
    }
    waves.sort_by_key(|w| w.number);
    waves
}

/// O número de um diretório `wave-<N>-<papel>`; `None` para qualquer outro.
fn wave_number(name: &str) -> Option<u32> {
    let after = name.strip_prefix("wave-")?;
    let digits = after.find(|c: char| !c.is_ascii_digit())?;
    if digits == 0 || !after[digits..].starts_with('-') {
        return None;
    }
    after[..digits].parse().ok()
}

/// Os ids do `satisfies: [AC-1, AC-2]` do frontmatter de uma onda.
fn satisfies(text: &str) -> Vec<String> {
    frontmatter_value(text, "satisfies")
        .map(|value| {
            value
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|s| s.trim().trim_matches(['"', '\'']).to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// O valor de `key:` no frontmatter `---` do começo do arquivo, sem aspas nas
/// pontas. `None` sem frontmatter, sem a chave ou com valor vazio.
fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return None;
    }
    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix(':')) {
            let value = rest.trim().trim_matches(['"', '\'']).trim();
            return (!value.is_empty()).then(|| value.to_string());
        }
    }
    None
}

/// O corpo de uma seção `## ` (título fora), pelo resolvedor compartilhado de
/// títulos — pt-BR e inglês resolvem igual. Vazio quando a seção falta.
fn section_body(text: &str, key: &str) -> String {
    section_block(text, key)
        .and_then(|block| block.split_once('\n').map(|(_, body)| body.to_string()))
        .unwrap_or_default()
}

fn first_paragraph(body: &str) -> &str {
    body.split("\n\n").map(str::trim).find(|p| !p.is_empty()).unwrap_or("")
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// O título da spec: o primeiro `# ` do `spec.md`.
fn spec_title(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// O título curto do cabeçalho. O `# ` de uma spec costuma ser o pedido
/// inteiro, longo demais para um `<h1>`. Vale, nesta ordem: o `title:` do
/// frontmatter; o trecho do `# ` antes do primeiro `: `; o `# ` inteiro. O
/// segundo valor é o pedido completo quando o título foi encurtado — a página o
/// mostra logo abaixo, para nada do pedido se perder.
fn short_title(spec_text: &str, heading: &str) -> (String, Option<String>) {
    if let Some(title) = frontmatter_value(spec_text, "title") {
        let rest = (title != heading).then(|| heading.to_string());
        return (title, rest);
    }
    match heading.split_once(": ") {
        Some((head, _)) if !head.trim().is_empty() && head.len() < heading.len() => {
            (head.trim().to_string(), Some(heading.to_string()))
        }
        _ => (heading.to_string(), None),
    }
}

/// O idioma da página: o do texto do projeto (`language.text`), em que a spec
/// é escrita. A spec não guarda um idioma próprio para concorrer com ele.
fn i18n_for(root: &Path) -> I18n {
    I18n::new(mustard_core::ProjectConfig::load(root).language().text_or_default())
}

/// O branch da unidade, quando o checkout está nele. Fora dele a página não
/// adivinha um nome.
fn unit_branch(root: &Path, slug: &str) -> Option<String> {
    let branch = mustard_core::current_branch(root)?;
    let config = mustard_core::ProjectConfig::load(root);
    (slug_of_work_branch(&branch, &config).as_deref() == Some(slug)).then_some(branch)
}

/// A data do último checkpoint, só o dia — `10/09/2026` em pt-BR, ISO em
/// inglês. Só o dia para a página não mudar a cada evento do mesmo dia.
fn display_date(checkpoint: &str, lang: Locale) -> Option<String> {
    let date = checkpoint.trim().get(..10)?;
    let mut parts = date.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    let shaped = year.len() == 4 && month.len() == 2 && day.len() == 2;
    if !shaped || !date.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        return None;
    }
    Some(match lang {
        Locale::PtBr => format!("{day}/{month}/{year}"),
        Locale::EnUs => date.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Onde a unidade está
// ---------------------------------------------------------------------------

/// Em que ponto dos sete passos a unidade está.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    Analyze,
    AwaitingApproval,
    Approved,
    Execute,
    Review,
    Verify,
    Close,
    Completed,
}

impl Position {
    /// Lê o estágio do `meta.json`; o marcador de aprovação separa um plano que
    /// espera o usuário de um que ele já aprovou.
    fn of(meta: &Meta, approved: bool) -> Self {
        if meta.outcome.as_deref().is_some_and(|o| o.eq_ignore_ascii_case("completed")) {
            return Self::Completed;
        }
        let stage: String = meta
            .stage
            .as_deref()
            .unwrap_or("")
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase();
        match stage.as_str() {
            "analyze" => Self::Analyze,
            "approved" => Self::Approved,
            "execute" | "implementing" | "inprogress" => Self::Execute,
            "reviewpending" | "review" | "reviewing" | "qareview" => Self::Review,
            "qapending" | "qa" => Self::Verify,
            "close" => Self::Close,
            // Plano, rascunho ou nada registrado: o marcador decide.
            _ if approved => Self::Approved,
            _ => Self::AwaitingApproval,
        }
    }

    /// O passo atual, de 1 a 7; `None` quando a unidade fechou.
    fn step(self) -> Option<usize> {
        match self {
            Self::Analyze => Some(1),
            Self::AwaitingApproval => Some(2),
            Self::Approved | Self::Execute => Some(4),
            Self::Review => Some(5),
            Self::Verify => Some(6),
            Self::Close => Some(7),
            Self::Completed => None,
        }
    }

    fn stage_key(self) -> &'static str {
        match self {
            Self::Analyze => "doc.stage.analyze",
            Self::AwaitingApproval => "doc.stage.awaiting",
            Self::Approved => "doc.stage.approved",
            Self::Execute => "doc.stage.execute",
            Self::Review => "doc.stage.review",
            Self::Verify => "doc.stage.verify",
            Self::Close => "doc.stage.close",
            Self::Completed => "doc.stage.completed",
        }
    }
}

// ---------------------------------------------------------------------------
// A página
// ---------------------------------------------------------------------------

/// A página mostra a spec como aprovada: o estado dela, no `spec.ndjson`.
pub(crate) fn is_approved(root: &Path, slug: &str) -> bool {
    crate::shared::spec_state::approved(root, slug)
}

fn render(root: &Path, slug: &str, dir: &Path) -> String {
    let meta = read_meta(dir);
    let i18n = i18n_for(root);
    let t = |key: &str| i18n.render(key);
    let position = Position::of(&meta, is_approved(root, slug));
    let spec_text = std::fs::read_to_string(dir.join("spec.md")).unwrap_or_default();
    let material = read_material(dir).unwrap_or_default();
    let waves = read_waves(dir, &meta);

    let heading = spec_title(&spec_text).unwrap_or_else(|| slug.to_string());
    let (title, full_request) = short_title(&spec_text, &heading);
    let kind = if position == Position::AwaitingApproval {
        "doc.kind.approval"
    } else {
        "doc.kind.summary"
    };
    let mut report = Report::new(title, "")
        .with_lang(i18n.lang.as_str())
        .with_kind(t(kind))
        .with_meta(&t("doc.meta.spec"), slug);
    if let Some(branch) = unit_branch(root, slug) {
        report = report.with_meta(&t("doc.meta.branch"), &branch);
    }
    if let Some(base) = meta.base.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        report = report.with_meta(&t("doc.meta.base"), base);
    }
    report = report.with_note(&t(position.stage_key()));
    if let Some(date) = meta.checkpoint.as_deref().and_then(|c| display_date(c, i18n.lang)) {
        report = report.with_note(&date);
    }

    if let Some(request) = full_request {
        report.raw(&format!("<p class=\"muted\">{}</p>", inline(&request)));
    }
    if let Some(html) = summary_html(&material, &i18n) {
        report.raw(&html);
    }
    report.section(&t("doc.section.where"), &steps_html(position, &i18n));
    if let Some(html) = clarified_html(&material, &i18n) {
        report.section(&t("doc.section.clarified"), &html);
    }
    if let Some(html) = decisions_html(&material) {
        report.section(&t("doc.section.decisions"), &html);
    }
    if let Some(html) = risks_html(&material, &i18n) {
        report.section(&t("doc.section.risks"), &html);
    }
    if let Some(html) = flow_html(&material) {
        report.section(&t("doc.section.flow"), &html);
    }
    if let Some(html) = spec_html(&spec_text, &i18n) {
        report.section(&t("doc.section.spec"), &html);
    }
    if let Some(html) = criteria_html(&spec_text, dir, &waves, &i18n) {
        report.section(&t("doc.section.criteria"), &html);
    }
    if let Some(html) = waves_html(root, &waves, &i18n) {
        report.section(&t("doc.section.waves"), &html);
    }
    if let Some(html) = evidence_html(&material, &i18n) {
        report.section(&t("doc.section.evidence"), &html);
    }
    if let Some(html) = pending_html(root, &i18n) {
        report.section(&t("doc.section.pending"), &html);
    }
    report.section(&t("doc.section.next"), &next_html(position, &waves, &i18n));
    report.raw(&format!("<footer>{}</footer>", escape(&t("doc.footer"))));
    report.render()
}

fn summary_html(material: &Material, i: &I18n) -> Option<String> {
    let summary = material.summary.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
    let mut paragraphs = summary.split("\n\n").map(str::trim).filter(|p| !p.is_empty());
    let first = paragraphs.next()?;
    let mut html = format!(
        "<div class=\"callout\"><p class=\"lead\">{}</p>",
        inline(&format!("**{}** {}", i.render("doc.summary.lead"), one_line(first))),
    );
    for paragraph in paragraphs {
        let _ = write!(html, "<p>{}</p>", inline(&one_line(paragraph)));
    }
    html.push_str("</div>");
    Some(html)
}

fn steps_html(position: Position, i: &I18n) -> String {
    let here = position.step();
    let mut html = String::from("<ol class=\"steps\">");
    for (index, key) in STEPS.iter().enumerate() {
        let n = index + 1;
        let class = match here {
            None => " class=\"done\"",
            Some(h) if n < h => " class=\"done\"",
            Some(h) if n == h => " class=\"here\"",
            Some(_) => "",
        };
        let _ = write!(
            html,
            "<li{class}><b>{}</b> — {}",
            escape(&i.render(&format!("doc.step.{key}.name"))),
            inline(&i.render(&format!("doc.step.{key}.desc"))),
        );
        if here == Some(n) {
            let _ = write!(html, " {}", escape(&i.render("doc.step.here")));
        }
        html.push_str("</li>");
    }
    html.push_str("</ol>");
    html
}

/// As perguntas respondidas e os termos definidos na conversa.
fn clarified_html(material: &Material, i: &I18n) -> Option<String> {
    let mut rows: Vec<Vec<(&str, String)>> = Vec::new();
    for c in &material.clarifications {
        let mut answer = inline(&c.answer);
        if let Some(notes) = c.notes.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            let _ = write!(
                answer,
                " <span class=\"muted\">{} {}</span>",
                escape(&i.render("doc.clarified.notes")),
                inline(notes),
            );
        }
        rows.push(vec![("", inline(&c.question)), ("", answer)]);
    }
    for d in &material.definitions {
        let question = i.render("doc.clarified.definition").replace("{term}", &format!("`{}`", d.term));
        rows.push(vec![("", inline(&question)), ("", inline(&d.meaning))]);
    }
    if rows.is_empty() {
        return None;
    }
    Some(rich_table(&[i.render("doc.col.question"), i.render("doc.col.answer")], &rows))
}

fn decisions_html(material: &Material) -> Option<String> {
    if material.decisions.is_empty() {
        return None;
    }
    let mut html = String::from("<ol class=\"wrap-code\">");
    for d in &material.decisions {
        let line = format!("**{}** {}", with_period(&d.decision), d.reason);
        let _ = write!(html, "<li>{}</li>", inline(&line));
    }
    html.push_str("</ol>");
    Some(html)
}

fn risks_html(material: &Material, i: &I18n) -> Option<String> {
    if material.risks.is_empty() {
        return None;
    }
    let mut risks: Vec<_> = material.risks.iter().collect();
    // Os mais graves primeiro; a ordem de gravação desempata.
    risks.sort_by_key(|r| severity_rank(r.severity));
    let rows: Vec<Vec<(&str, String)>> = risks
        .iter()
        .map(|r| {
            let class = if r.severity == Severity::Alta { "sev-alta nw" } else { "nw" };
            let label = i.render(&format!("doc.severity.{}", r.severity.as_str()));
            vec![(class, escape(&label)), ("", inline(&r.risk)), ("", inline(&r.mitigation))]
        })
        .collect();
    Some(rich_table(
        &[i.render("doc.col.severity"), i.render("doc.col.risk"), i.render("doc.col.mitigation")],
        &rows,
    ))
}

/// O antes e depois: o título como rótulo e o diagrama num `<pre>`, escapado e
/// com o recuo intacto — um diagrama em texto alinha pelas colunas.
fn flow_html(material: &Material) -> Option<String> {
    let flow = material.flow.as_ref()?;
    let diagram = flow.diagram.trim_end();
    if diagram.trim().is_empty() {
        return None;
    }
    let mut html = String::new();
    let title = flow.title.trim();
    if !title.is_empty() {
        let _ = write!(html, "<p class=\"label\">{}</p>", inline(title));
    }
    let _ = write!(html, "<pre>{}</pre>", escape(diagram));
    Some(html)
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Alta => 0,
        Severity::Media => 1,
        Severity::Baixa => 2,
    }
}

/// Contexto, métrica, não-objetivos e limites da spec, e os arquivos dela.
fn spec_html(spec_text: &str, i: &I18n) -> Option<String> {
    let mut dl = String::new();
    for (key, label) in [
        ("context", "heading.spec.context"),
        ("metric", "heading.spec.metric"),
        ("non-goals", "heading.spec.non_goals"),
        ("boundaries", "heading.spec.limits"),
    ] {
        let body = blocks_html(&section_body(spec_text, key));
        if body.is_empty() {
            continue;
        }
        let _ = write!(
            dl,
            "<div class=\"wrap-code\"><dt>{}</dt><dd>{body}</dd></div>",
            escape(&i.render(label)),
        );
    }
    let mut html = String::new();
    if !dl.is_empty() {
        let _ = write!(html, "<dl>{dl}</dl>");
    }
    let files = arquivos_paths(spec_text);
    if !files.is_empty() {
        let _ = write!(
            html,
            "<p class=\"label\">{}</p>{}",
            escape(&i.render("heading.spec.files")),
            files_list(&files),
        );
    }
    (!html.is_empty()).then_some(html)
}

/// Os critérios de aceite, lidos pelo MESMO parser que o QA executa, com a
/// onda que satisfaz cada um.
fn criteria_html(spec_text: &str, dir: &Path, waves: &[WaveDoc], i: &I18n) -> Option<String> {
    let mut items = extract_ac_section(spec_text).map(|s| parse_ac_items(&s)).unwrap_or_default();
    if items.is_empty() {
        // Um plano em ondas carrega os critérios também no `wave-plan.md`.
        let plan = std::fs::read_to_string(dir.join("wave-plan.md")).unwrap_or_default();
        items = extract_ac_section(&plan).map(|s| parse_ac_items(&s)).unwrap_or_default();
    }
    if items.is_empty() {
        return None;
    }
    let rows: Vec<Vec<(&str, String)>> = items
        .iter()
        .map(|item| {
            let waves_for: Vec<String> = waves
                .iter()
                .filter(|w| w.satisfies.contains(&item.id))
                .map(|w| w.number.to_string())
                .collect();
            vec![
                ("id", escape(&item.id)),
                ("", inline(&item.statement)),
                ("nw", escape(&waves_for.join(", "))),
            ]
        })
        .collect();
    Some(format!(
        "<p>{}</p>{}",
        inline(&i.render("doc.criteria.lead")),
        rich_table(
            &[
                i.render("doc.col.id"),
                i.render("doc.col.criterion"),
                i.render("doc.col.wave"),
            ],
            &rows,
        ),
    ))
}

fn waves_html(root: &Path, waves: &[WaveDoc], i: &I18n) -> Option<String> {
    if waves.is_empty() {
        return None;
    }
    let mut html = format!("<p>{}</p>", inline(&i.render("doc.waves.lead")));
    for w in waves {
        let _ = write!(html, "<h3><span class=\"tag\">{}</span>", escape(&wave_label(w.number, i.lang)));
        if !w.summary.is_empty() {
            let _ = write!(html, " · {}", inline(&w.summary));
        }
        if w.done {
            let _ = write!(html, " <span class=\"muted\">· {}</span>", escape(&i.render("doc.wave.done")));
        }
        html.push_str("</h3>");
        html.push_str(&blocks_html(&w.tasks));
        if !w.files.is_empty() {
            let _ = write!(
                html,
                "<p class=\"label\">{}</p>{}",
                escape(&i.render("heading.spec.files")),
                files_list(&w.files),
            );
        }
        let covers = mold_covers(root, &w.files);
        if !covers.is_empty() {
            let _ = write!(
                html,
                "<p class=\"label\">{}</p><ul class=\"wrap-code\">",
                escape(&i.render("doc.wave.skills")),
            );
            let covers_word = i.render("doc.wave.covers");
            for cover in &covers {
                let files: Vec<String> = cover.files.iter().map(|f| format!("`{f}`")).collect();
                let line = format!("`{}` — {covers_word} {}", cover.name, files.join(", "));
                let _ = write!(html, "<li>{}</li>", inline(&line));
            }
            html.push_str("</ul>");
        }
        if !w.obligations.trim().is_empty() {
            let _ = write!(
                html,
                "<p class=\"label\">{}</p>{}",
                escape(&i.render("doc.wave.obligations")),
                blocks_html(&w.obligations),
            );
        }
        if !w.satisfies.is_empty() {
            let _ = write!(
                html,
                "<p><span class=\"label\">{}:</span> {}</p>",
                escape(&i.render("doc.wave.criteria")),
                escape(&w.satisfies.join(", ")),
            );
        }
    }
    Some(html)
}

/// Os moldes que governam os arquivos de uma onda, de todos os subprojetos
/// que esses arquivos tocam: o subprojeto de um arquivo é a pasta mais próxima,
/// subindo a partir dele, que tem `.claude/skills/`. O cruzamento em si é o do
/// prompt da onda (`wave_molds`), então página e prompt nomeiam os mesmos.
fn mold_covers(root: &Path, files: &[String]) -> Vec<MoldCover> {
    let mut subprojects: BTreeSet<String> = BTreeSet::new();
    for file in files {
        let mut dir = Path::new(file).parent();
        while let Some(d) = dir {
            if root.join(d).join(".claude").join("skills").is_dir() {
                subprojects.insert(d.to_string_lossy().replace('\\', "/"));
                break;
            }
            dir = d.parent();
        }
    }
    let mut covers: Vec<MoldCover> =
        subprojects.iter().flat_map(|s| wave_molds(root, s, files)).collect();
    covers.sort_by(|a, b| a.name.cmp(&b.name));
    covers
}

fn evidence_html(material: &Material, i: &I18n) -> Option<String> {
    if material.findings.is_empty() {
        return None;
    }
    let rows: Vec<Vec<(&str, String)>> = material
        .findings
        .iter()
        .map(|f| {
            let place = match f.line {
                Some(line) => format!("{}:{line}", f.file),
                None => f.file.clone(),
            };
            vec![("", inline(&f.statement)), ("where", breakable_path(&escape(&place)))]
        })
        .collect();
    Some(rich_table(&[i.render("doc.col.seen"), i.render("doc.col.where")], &rows))
}

fn pending_html(root: &Path, i: &I18n) -> Option<String> {
    let open = open_pending(root);
    if open.is_empty() {
        return None;
    }
    let rows: Vec<Vec<(&str, String)>> =
        open.iter().map(|p| vec![("id", escape(&p.id)), ("", inline(&p.title))]).collect();
    Some(rich_table(&[i.render("doc.col.id"), i.render("doc.col.pending")], &rows))
}

fn next_html(position: Position, waves: &[WaveDoc], i: &I18n) -> String {
    let lines: Vec<String> = match position {
        Position::Analyze => vec![i.render("doc.next.analyze")],
        Position::AwaitingApproval => vec![i.render("doc.next.approve"), i.render("doc.next.adjust")],
        Position::Approved => vec![i.render("doc.next.approved")],
        Position::Execute => match waves.iter().find(|w| !w.done) {
            Some(w) => vec![i.render("doc.next.execute").replace("{wave}", &w.number.to_string())],
            None => vec![i.render("doc.next.execute_done")],
        },
        Position::Review | Position::Verify => vec![i.render("doc.next.review")],
        Position::Close => vec![i.render("doc.next.close")],
        Position::Completed => vec![i.render("doc.next.completed")],
    };
    let mut html = String::from("<div class=\"callout\">");
    for line in lines {
        let _ = write!(html, "<p>{}</p>", inline(&line));
    }
    html.push_str("</div>");
    html
}

// ---------------------------------------------------------------------------
// Pequenos montadores de HTML
// ---------------------------------------------------------------------------

/// Texto de usuário para HTML, pelo conversor do motor de página: código
/// entre crases, negrito entre `**` e o resto escapado.
fn inline(text: &str) -> String {
    crate::report::markdown::inline(text, &BTreeSet::new())
}

/// Um trecho de markdown da spec em HTML, pelo conversor do motor de página.
/// Vazio quando não sobra nada.
fn blocks_html(body: &str) -> String {
    crate::report::markdown_html(body)
}

fn with_period(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.ends_with(['.', '!', '?', ':', ';', '…']) {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

fn files_list(files: &[String]) -> String {
    let mut html = String::from("<ul class=\"files\">");
    for file in files {
        let _ = write!(html, "<li>{}</li>", escape(file));
    }
    html.push_str("</ul>");
    html
}

/// Um caminho ou endereço já escapado, com a barra como ÚNICO ponto de quebra:
/// cada trecho (até a sua `/`, inclusive) vai num `<span class="nw">` que não
/// quebra, e um `<wbr>` separa um trecho do seguinte. Só o `<wbr>` não bastava:
/// com a célula em `white-space:normal`, o navegador também quebrava depois do
/// hífen de um nome de pasta (`2026-09-10-pagina-…`). O `//` de um endereço fica
/// inteiro, dentro do mesmo trecho.
fn breakable_path(escaped: &str) -> String {
    let mut out = String::with_capacity(escaped.len() * 2);
    let mut segment = String::new();
    let mut chars = escaped.chars().peekable();
    while let Some(c) = chars.next() {
        segment.push(c);
        if c == '/' && chars.peek().is_some_and(|next| *next != '/') {
            let _ = write!(out, "<span class=\"nw\">{segment}</span><wbr>");
            segment.clear();
        }
    }
    if !segment.is_empty() {
        let _ = write!(out, "<span class=\"nw\">{segment}</span>");
    }
    out
}

/// Uma tabela no molde do layout: cabeçalho escapado e células com HTML já
/// montado, cada uma com a sua classe (vazia quando não tem).
fn rich_table(headers: &[String], rows: &[Vec<(&str, String)>]) -> String {
    let mut html = String::from("<div class=\"table\"><table><thead><tr>");
    for header in headers {
        let _ = write!(html, "<th>{}</th>", escape(header));
    }
    html.push_str("</tr></thead><tbody>");
    for row in rows {
        html.push_str("<tr>");
        for (class, cell) in row {
            if class.is_empty() {
                let _ = write!(html, "<td>{cell}</td>");
            } else {
                let _ = write!(html, "<td class=\"{class}\">{cell}</td>");
            }
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table></div>");
    html
}

/// Relativo ao repositório, com barras normais: o relatório lê igual em toda
/// plataforma.
fn repo_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// O `file://` absoluto da página, o link que o usuário clica.
fn file_url(path: &Path) -> String {
    let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut text = absolute.to_string_lossy().replace('\\', "/");
    // O prefixo `\\?\` do Windows não pertence a uma URL.
    if let Some(rest) = text.strip_prefix("//?/") {
        text = rest.to_string();
    }
    let mut url = String::from("file://");
    if !text.starts_with('/') {
        url.push('/');
    }
    for c in text.chars() {
        match c {
            ' ' => url.push_str("%20"),
            '#' => url.push_str("%23"),
            '?' => url.push_str("%3F"),
            '%' => url.push_str("%25"),
            _ => url.push(c),
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const SPEC: &str = "---\nid: spec.demo\n---\n\n# Resumo legível da spec\n\n\
## Contexto\n\nO usuário não consegue ler a spec no terminal.\n\n\
## Métrica de sucesso\n\nToda spec em espera de aprovação tem um `resumo.html`.\n\n\
## Não-Objetivos\n\nMudar o painel web.\n\n\
## Critérios de Aceitação\n\n\
- AC-1 — when o documento é montado, then traz todas as seções. Command: `cargo test -p demo alpha`\n\
- AC-2 — when uma skill cobre vários arquivos, then nomeia todos. Command: `cargo test -p demo beta`\n\
- AC-3 — when as ondas terminam, then compila. Command: `cargo build -p demo`\n\n\
## Arquivos\n\n- `apps/rt/src/commands/spec/spec_doc.rs`\n- `apps/rt/src/commands/spec/cli.rs`\n\n\
## Limites\n\nIN: o comando. OUT: o painel.\n";

    const WAVE: &str = "---\nid: wave.demo.1-doc\nsatisfies: [AC-1, AC-2]\n---\n\n# wave-1-doc\n\n\
## Summary\n\nO comando que monta o documento\n\n\
## Tasks\n\n- [ ] Novo comando `spec-doc` que monta a página.\n\n\
## Files\n\n- `apps/rt/src/commands/spec/spec_doc.rs`\n- `apps/rt/src/commands/spec/cli.rs`\n\n\
## Reality Obligations\n\n- **RO-1.1** — Conferir como o navegador abre um `file://` local.\n";

    const MATERIAL: &str = r#"{
  "definitions": [{"term": "onda", "meaning": "uma etapa de execução"}],
  "decisions": [{"decision": "O layout v4 é o padrão", "reason": "Aprovado pelo usuário"}],
  "findings": [{"statement": "o Report já existe", "file": "apps/rt/src/report/mod.rs", "line": 97}],
  "risks": [
    {"risk": "o texto fica longo", "mitigation": "seções recolhem", "severity": "baixa"},
    {"risk": "o navegador abre sem pedir", "mitigation": "um interruptor desliga", "severity": "alta"}
  ],
  "clarifications": [{"question": "Abrir sozinho?", "answer": "Só na aprovação", "notes": "uma vez por versão"}],
  "summary": "A conversa pediu um HTML legível.",
  "flow": {"title": "Entrega do documento", "diagram": "  antes: terminal <texto>\n    |\n  depois: página"}
}"#;
    const PENDING: &str = r#"{"items": [
  {"id": "P-1", "title": "Humanize: medir se o texto está claro", "detail": "combinado", "status": "open"},
  {"id": "P-2", "title": "item já fechado", "detail": "x", "status": "closed", "reason": "entregue"}
]}"#;

    /// Uma unidade com tudo o que a página lê: spec, meta, material, prova,
    /// uma onda, um molde e a lista de pendências.
    fn seed(root: &Path) {
        fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
        let dir = root.join(".claude/spec/demo");
        fs::create_dir_all(dir.join("wave-1-doc")).unwrap();
        fs::write(dir.join("spec.md"), SPEC).unwrap();
        fs::write(
            dir.join("meta.json"),
            r#"{"stage":"Plan","outcome":"Active","lang":"pt-BR","checkpoint":"2026-09-10T10:00:00.000Z","base":"dev"}"#,
        )
        .unwrap();
        fs::write(dir.join("spec-material.json"), MATERIAL).unwrap();
        fs::write(dir.join("wave-1-doc/spec.md"), WAVE).unwrap();
        let mold = root.join("apps/rt/.claude/skills/rt-entry-pattern");
        fs::create_dir_all(&mold).unwrap();
        fs::write(
            mold.join("SKILL.md"),
            "---\nname: rt-entry-pattern\ndescription: \"Use when adding a run command.\"\n\
             paths:\n  - apps/rt/src/commands/**\nsource: scan\n---\n\nbody\n",
        )
        .unwrap();
        fs::create_dir_all(root.join(".claude/pending")).unwrap();
        fs::write(root.join(".claude/pending/ledger.json"), PENDING).unwrap();
    }

    /// A página de uma spec com material, ondas, prova e pendências traz
    /// cada seção, na ordem combinada, e só regrava quando o conteúdo muda.
    #[test]
    fn spec_doc_renders_every_section() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        seed(root);

        let report = generate(root, "demo");
        assert!(report.ok && report.changed, "{report:?}");
        assert_eq!(report.path, ".claude/spec/demo/resumo.html");
        assert!(report.url.starts_with("file:///") && report.url.ends_with("/resumo.html"), "{}", report.url);
        assert_eq!(report.hash.len(), 16);
        let html = fs::read_to_string(root.join(".claude/spec/demo").join(DOC_FILE)).unwrap();
        assert!(!html.contains("<missing-key>"), "every text comes from the catalogue:\n{html}");
        assert!(html.contains("<html lang=\"pt-BR\">"));

        // Cabeçalho: título, spec, base, estágio e data.
        for needle in [
            "<h1>Resumo legível da spec</h1>",
            "Mustard · spec para aprovar",
            "spec <b>demo</b>",
            "sai de <b>dev</b>",
            "<li>aguardando aprovação</li>",
            "<li>10/09/2026</li>",
        ] {
            assert!(html.contains(needle), "header misses {needle}:\n{html}");
        }

        // As seções, na ordem combinada.
        let order = [
            "A conversa pediu um HTML legível.",
            "<h2>Onde estamos</h2>",
            "<h2>O que foi esclarecido</h2>",
            "<h2>Decisões</h2>",
            "<h2>Riscos</h2>",
            "<h2>Antes e depois</h2>",
            "<h2>A spec</h2>",
            "<h2>Critérios de aceite</h2>",
            "<h2>Ondas e skills</h2>",
            "<h2>Evidências</h2>",
            "<h2>Pendências abertas</h2>",
            "<h2>Próximo passo</h2>",
            "<footer>",
        ];
        let mut last = 0;
        for needle in order {
            let at = html.find(needle).unwrap_or_else(|| panic!("missing {needle}:\n{html}"));
            assert!(at >= last, "{needle} is out of order");
            last = at;
        }

        // Onde estamos: a análise passou e o plano é o passo atual.
        assert!(html.contains("<li class=\"done\"><b>Análise</b>"), "{html}");
        assert!(html.contains("<li class=\"here\"><b>Plano</b>"), "{html}");

        for needle in [
            // Esclarecimentos: pergunta, resposta e nota; o termo definido.
            "Abrir sozinho?",
            "Só na aprovação",
            "uma vez por versão",
            "O que quer dizer <code>onda</code> aqui?",
            "uma etapa de execução",
            // Decisões com motivo; riscos com gravidade e atenuante.
            "<strong>O layout v4 é o padrão.</strong> Aprovado pelo usuário",
            "<td class=\"sev-alta nw\">Alta</td><td>o navegador abre sem pedir</td><td>um interruptor desliga</td>",
            // A spec.
            "O usuário não consegue ler a spec no terminal.",
            "Mudar o painel web.",
            "IN: o comando. OUT: o painel.",
            "<ul class=\"files\"><li>apps/rt/src/commands/spec/cli.rs</li>",
            // Critérios com a onda que os satisfaz.
            "<td class=\"id\">AC-1</td>",
            // A onda: tarefas, skills com TODOS os arquivos, obrigação, critérios.
            "<span class=\"tag\">Onda 1</span> · O comando que monta o documento",
            "Novo comando <code>spec-doc</code> que monta a página.",
            "<code>rt-entry-pattern</code> — cobre <code>apps/rt/src/commands/spec/cli.rs</code>, \
             <code>apps/rt/src/commands/spec/spec_doc.rs</code>",
            "Obrigação externa",
            "<li><strong>RO-1.1</strong> — Conferir como o navegador abre um <code>file://</code> local.</li>",
            "<span class=\"label\">Critérios:</span> AC-1, AC-2",
            // Evidência com arquivo:linha; pendência aberta.
            "<td class=\"where\"><span class=\"nw\">apps/</span><wbr><span class=\"nw\">rt/</span><wbr>\
             <span class=\"nw\">src/</span><wbr><span class=\"nw\">report/</span><wbr>\
             <span class=\"nw\">mod.rs:97</span></td>",
            "<td class=\"id\">P-1</td><td>Humanize: medir se o texto está claro</td>",
            // Próximo passo pelo estágio.
            "Para aprovar, digite <code>/mustard:spec</code> neste branch.",
        ] {
            assert!(html.contains(needle), "missing {needle}:\n{html}");
        }
        // O risco alto vem antes do baixo, e a pendência fechada não aparece.
        assert!(html.find("o navegador abre sem pedir") < html.find("o texto fica longo"));
        assert!(!html.contains("item já fechado"), "a closed pending item is not listed");

        // A mesma entrada gera a mesma página: nada é regravado.
        let again = generate(root, "demo");
        assert!(again.ok && !again.changed, "{again:?}");
        assert_eq!(again.hash, report.hash);
    }

    /// Na tabela de Evidências, a coluna Onde quebra a linha só depois
    /// de cada barra: cada trecho do caminho vai num `nowrap` inteiro (nem o
    /// hífen de um nome de pasta quebra), o `<wbr>` fica só ENTRE trechos, e o
    /// `//` de um endereço não se parte. E a regra `td.where` do layout deixou de
    /// proibir a quebra de linha, senão os `<wbr>` não serviriam para nada.
    #[test]
    fn evidence_location_wraps_at_path_separators() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        seed(root);
        let findings = r#"[
    {"statement": "o caminho inteiro espremia a coluna", "file": "apps/rt/src/commands/spec/spec_doc.rs", "line": 863},
    {"statement": "a página publicada", "file": "https://claude.ai/code/artifacts/demo"},
    {"statement": "a pasta com hífens", "file": ".claude/spec/2026-09-10-pagina-spec-sempre-publicada/resumo.html"}
  ]"#;
        let material = MATERIAL.replace(
            r#"[{"statement": "o Report já existe", "file": "apps/rt/src/report/mod.rs", "line": 97}]"#,
            findings,
        );
        assert_ne!(material, MATERIAL, "the fixture must carry the long findings");
        fs::write(root.join(".claude/spec/demo/spec-material.json"), material).unwrap();

        assert!(generate(root, "demo").ok);
        let html = fs::read_to_string(root.join(".claude/spec/demo").join(DOC_FILE)).unwrap();

        const OPEN: &str = "<span class=\"nw\">";
        for segment in [
            "<span class=\"nw\">2026-09-10-pagina-spec-sempre-publicada/</span><wbr>",
            "<span class=\"nw\">https://</span><wbr><span class=\"nw\">claude.ai/</span><wbr>",
            "<span class=\"nw\">spec/</span><wbr><span class=\"nw\">spec_doc.rs:863</span></td>",
        ] {
            assert!(html.contains(segment), "missing {segment}:\n{html}");
        }
        let cells: Vec<&str> = html
            .split("<td class=\"where\">")
            .skip(1)
            .filter_map(|rest| rest.split_once("</td>").map(|(cell, _)| cell))
            .collect();
        assert_eq!(cells.len(), 3, "{cells:?}");
        for (cell, place) in cells.iter().zip([
            "apps/rt/src/commands/spec/spec_doc.rs:863",
            "https://claude.ai/code/artifacts/demo",
            ".claude/spec/2026-09-10-pagina-spec-sempre-publicada/resumo.html",
        ]) {
            // O `<wbr>` só ENTRE trechos: cada pedaço entre dois `<wbr>` é
            // exatamente um span `nw`, sem outra marca dentro.
            let pieces: Vec<&str> = cell
                .split("<wbr>")
                .map(|piece| {
                    piece
                        .strip_prefix(OPEN)
                        .and_then(|rest| rest.strip_suffix("</span>"))
                        .filter(|text| !text.contains('<'))
                        .unwrap_or_else(|| panic!("a piece outside a nowrap span: {piece:?} in {cell}"))
                })
                .collect();
            // Os trechos, juntos, são o caminho inteiro: nada fica fora de um span.
            assert_eq!(pieces.concat(), place, "{cell}");
            // Cada trecho termina na sua barra, e barra só no fim (ou no `//`).
            let (_, before_last) = pieces.split_last().expect("at least one piece");
            for piece in before_last {
                assert!(piece.ends_with('/'), "a break that is not after a slash: {cell}");
            }
            for piece in &pieces {
                // O `//` sai primeiro: `https://` é um trecho só, inteiro.
                let body = piece.replace("//", "");
                let body = body.strip_suffix('/').unwrap_or(&body);
                assert!(!body.contains('/'), "a slash without a break after it: {cell}");
            }
        }

        let rule = |selector: &str| {
            html.split_once(selector)
                .and_then(|(_, tail)| tail.split_once('}'))
                .map(|(decls, _)| decls.to_string())
                .unwrap_or_else(|| panic!("layout without a {selector} rule"))
        };
        let place = rule("td.where{");
        assert!(!place.contains("white-space:nowrap"), "td.where still forbids wrapping: {place}");
        assert!(rule(".nw{").contains("white-space:nowrap"), "the .nw span must not wrap");
    }

    /// O `flow` do material vira a seção Antes e depois, entre os riscos
    /// e a spec, com o diagrama num bloco monoespaçado, escapado e com o recuo
    /// intacto; sem `flow`, a seção some.
    #[test]
    fn a_flow_material_renders_before_and_after() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        seed(root);
        let dir = root.join(".claude/spec/demo");

        assert!(generate(root, "demo").ok);
        let html = fs::read_to_string(dir.join(DOC_FILE)).unwrap();
        let at = html.find("<h2>Antes e depois</h2>").unwrap_or_else(|| panic!("no section:\n{html}"));
        let risks = html.find("<h2>Riscos</h2>").unwrap();
        let spec = html.find("<h2>A spec</h2>").unwrap();
        assert!(risks < at && at < spec, "the section sits between the risks and the spec");
        assert!(
            html.contains(
                "<p class=\"label\">Entrega do documento</p>\
                 <pre>  antes: terminal &lt;texto&gt;\n    |\n  depois: página</pre>"
            ),
            "title, then the diagram escaped with its indentation:\n{html}",
        );

        // Sem `flow` no material, a seção não aparece.
        fs::write(dir.join("spec-material.json"), r#"{"summary": "Sem fluxo."}"#).unwrap();
        assert!(generate(root, "demo").ok);
        let html = fs::read_to_string(dir.join(DOC_FILE)).unwrap();
        assert!(!html.contains("Antes e depois"), "no flow, no section:\n{html}");
    }

    /// O cabeçalho usa o título curto: o `title:` do frontmatter, senão o
    /// trecho antes do primeiro `: `; o pedido inteiro vai logo abaixo.
    #[test]
    fn the_header_prefers_the_short_title() {
        let long = "HTML padrão da spec: documento autocontido que o Mustard gera";
        assert_eq!(
            short_title("# x\n", long),
            ("HTML padrão da spec".to_string(), Some(long.to_string())),
        );
        assert_eq!(
            short_title("---\ntitle: \"Resumo da spec\"\n---\n", long),
            ("Resumo da spec".to_string(), Some(long.to_string())),
        );
        assert_eq!(short_title("# x\n", "Sem dois-pontos"), ("Sem dois-pontos".to_string(), None));
    }

    /// Uma seção sem conteúdo some, e a aprovação muda o passo e o próximo passo.
    #[test]
    fn spec_doc_collapses_empty_sections_and_follows_the_stage() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let dir = root.join(".claude/spec/bare");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("meta.json"), r#"{"stage":"Execute"}"#).unwrap();
        // A página fala o idioma do texto do projeto.
        fs::write(root.join("mustard.json"), r#"{"language":{"text":"en-US"}}"#).unwrap();
        let report = generate(root, "bare");
        assert!(report.ok, "{report:?}");
        let html = fs::read_to_string(dir.join(DOC_FILE)).unwrap();
        assert!(html.contains("<html lang=\"en-US\">"));
        assert!(html.contains("<li class=\"here\"><b>Execution</b>"), "{html}");
        for absent in ["<h2>Risks</h2>", "<h2>Decisions</h2>", "<h2>Waves and skills</h2>", "callout\"><p class=\"lead\">"] {
            assert!(!html.contains(absent), "{absent} must collapse:\n{html}");
        }
        assert!(html.contains("Every wave is done; review comes next."), "{html}");
    }

    /// Um nome que não é spec é recusado pelo nome, sem gravar nada.
    #[test]
    fn spec_doc_refuses_an_unknown_or_invalid_spec() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(generate(tmp.path(), "nao-existe").error.as_deref(), Some("unknown_spec"));
        assert_eq!(generate(tmp.path(), "../fora").error.as_deref(), Some("invalid_spec"));
    }

    /// `--published-url` grava o endereço na pasta da spec, o relatório
    /// o devolve em `publishedUrl` e a retomada o devolve no mesmo campo. Gravar
    /// não muda a página — senão o gancho de fim de resposta pediria outra
    /// publicação —, e o que não é link é recusado sem mexer no arquivo.
    #[test]
    fn published_url_is_recorded_and_resume_reports_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        seed(root);

        let before = generate(root, "demo");
        assert!(before.ok && before.published_url.is_none(), "{before:?}");
        let json = serde_json::to_value(&before).unwrap();
        assert!(json.get("publishedUrl").is_some_and(serde_json::Value::is_null), "null when none: {json}");

        let url = "https://claude.ai/code/artifacts/demo-page";
        record_published_url(root, "demo", &format!("  {url}\n")).expect("records the address");
        let file = root.join(".claude/spec/demo").join(PUBLISHED_URL_FILE);
        assert_eq!(fs::read_to_string(&file).unwrap(), format!("{url}\n"));
        assert_eq!(published_url(root, "demo").as_deref(), Some(url));

        let after = generate(root, "demo");
        assert_eq!(after.published_url.as_deref(), Some(url));
        assert_eq!(serde_json::to_value(&after).unwrap()["publishedUrl"].as_str(), Some(url));
        assert!(!after.changed && after.hash == before.hash, "recording the address never changes the page");

        // A retomada lê o mesmo arquivo, pelo mesmo leitor.
        let resume = crate::commands::pipeline::resume_bootstrap::bootstrap(root, "demo");
        assert_eq!(serde_json::to_value(&resume).unwrap()["publishedUrl"].as_str(), Some(url));

        // Recusas: o arquivo fica como estava.
        for bad in ["claude.ai/code/artifacts/x", "https://", "https://a b", "ftp://x/y"] {
            let refusal = record_published_url(root, "demo", bad).unwrap_err();
            assert_eq!(refusal.0, "invalid_published_url", "{bad}");
        }
        assert_eq!(record_published_url(root, "nao-existe", url).unwrap_err().0, "unknown_spec");
        assert_eq!(record_published_url(root, "../fora", url).unwrap_err().0, "invalid_spec");
        assert_eq!(published_url(root, "demo").as_deref(), Some(url));
    }

    /// Caractere de controle no endereço é recusado com o mesmo erro, e nada é
    /// gravado: nem o arquivo novo, nem por cima de um endereço já gravado. Um
    /// ESC chegou a ser gravado ao vivo.
    #[test]
    fn published_url_refuses_control_characters() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        seed(root);
        let file = root.join(".claude/spec/demo").join(PUBLISHED_URL_FILE);
        let bad = [
            "https://claude.ai/code/artifacts/x\u{1b}[31m",
            "https://claude.ai/\u{7}x",
            "https://claude.ai/x\u{0}",
            "https://claude.ai/x\u{7f}",
        ];

        for url in bad {
            let refusal = record_published_url(root, "demo", url).unwrap_err();
            assert_eq!(refusal.0, "invalid_published_url", "{url:?}");
        }
        assert!(!file.exists(), "a refused address writes nothing");

        let good = "https://claude.ai/code/artifacts/demo-page";
        record_published_url(root, "demo", good).expect("records the address");
        for url in bad {
            assert!(record_published_url(root, "demo", url).is_err(), "{url:?}");
        }
        assert_eq!(fs::read_to_string(&file).unwrap(), format!("{good}\n"), "the recorded address stays");
    }
}
