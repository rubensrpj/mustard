//! As páginas de uma spec: a cópia dela para o banco de dados da página
//! publicada.
//!
//! A página publicada de uma spec, e a do projeto, são templates do Mustard
//! que leem o banco de dados guardado junto delas no claude.ai. O binário não
//! escreve mais o `spec.md`, o `spec.html` nem o `project.html` em passo
//! nenhum do fluxo: nos marcos (a aprovação, o fim de uma rodada e o
//! fechamento) e logo depois de um pedido que muda o plano, ele prepara a
//! cópia para o banco ([`copy`]), e o marco manda copiá-la, pela mesma porta,
//! [`end_milestone`]. A primeira vez de cada página, o marco manda antes
//! publicar o template dela e gravar o endereço, e a primeira cópia da spec,
//! que a leva inteira, fica com um agente separado. A spec antiga, cuja
//! página uma versão antiga publicou inteira, ganha o template num link novo,
//! e as tarefas das ondas dela que ainda não saíram ganham nota.
//!
//! O item que guarda um trecho com cara de segredo não vai para o banco, e
//! nem segura o marco: o marco diz o código dele para ser expurgado. A cópia
//! que não pôde ser preparada vai para os avisos, e o marco não manda copiar
//! nada: a cópia seguinte leva os mesmos itens.
//!
//! O comando que refazia o `spec.md`, o `spec.html` e o `project.html` a
//! partir do arquivo de eventos saiu, com o motor que só ele usava: a página
//! de uma spec e a do projeto só existem como template mais banco de dados.
//! O que fica é a conferência antes de publicar, comum a toda página: todo
//! trecho com cara de segredo sai dela como "…" ([`publishable`]), e a página
//! que passaria de [`PAGE_MAX_BYTES`] perde os registros mais antigos da
//! conversa.
//!
//! A lista dos itens sem dono (`owners.html`, ao lado da página da spec) sai
//! só quando alguém pede, pelo `page --spec <nome> --owners`, por
//! [`write_owners`].

pub(crate) mod copy;
pub(crate) mod secret;

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Refusal, SpecLog};
use mustard_core::domain::wave_prompt::OwnerLine;
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::view::document::{conversation_len, cut_oldest_conversation, owners_page, Document};
use mustard_core::ClaudePaths;

use serde_json::{json, Value};

use crate::report::Render;

/// O tamanho máximo, em bytes, de uma página que vai ser publicada: o
/// claude.ai aceita até 16 MB.
pub(crate) const PAGE_MAX_BYTES: usize = 16_000_000;

/// O nome da lista dos itens sem dono, ao lado da página da spec.
const OWNERS_PAGE: &str = "owners.html";

/// A página pronta para publicar e o que a conferência trocou nela.
struct Publishable {
    html: String,
    withheld: Vec<String>,
    loose: usize,
    trimmed: usize,
    too_big: bool,
}

/// A pasta é de uma spec do formato antigo, cujo `spec.md` é o documento e não
/// a página refeita do arquivo de eventos. Dois sinais, e a diferença entre
/// eles importa:
///
/// - o `spec.md` com a seção de critérios de aceitação, de onde o fluxo antigo
///   lia os critérios, marca a spec como antiga sempre;
/// - o `meta.json` marca só quando não há `spec.ndjson`: uma spec aberta pelo
///   `open` pode ganhar um `meta.json` de uma porta antiga e continua nova.
///
/// Numa pasta assim o binário não grava nada: nem o evento, nem a página, que
/// refeita do arquivo de eventos apagaria o texto da spec. A única conferência
/// disso.
pub(crate) fn old_format_spec(root: &Path, spec: &str) -> bool {
    let Ok(paths) = ClaudePaths::for_project(root).and_then(|paths| paths.for_spec(spec.trim())) else {
        return false;
    };
    let criteria_section = std::fs::read_to_string(paths.spec_md_path())
        .ok()
        .and_then(|md| crate::commands::review::qa_run::extract_ac_section(&md))
        .is_some();
    criteria_section || (paths.meta_json_path().is_file() && !paths.spec_ndjson_path().is_file())
}

/// A economia do rtk no projeto, só dos dias já fechados tanto na hora local
/// quanto na universal: o rtk pode contar o dia por qualquer uma delas.
pub(crate) fn rtk_days(root: &Path) -> Vec<mustard_core::view::document::RtkDay> {
    let local = mustard_core::io::spec_index::today();
    let universal = mustard_core::time::now_iso8601();
    let before = universal.get(..10).map_or(local.as_str(), |utc| utc.min(local.as_str()));
    crate::shared::rtk_gain::project_days(root, before)
}

/// O caminho do `spec.html` da spec `spec` do projeto `root`: a lista dos
/// itens sem dono é gravada ao lado dele.
fn spec_html_path(root: &Path, spec: &str) -> Result<PathBuf, Refusal> {
    let paths = ClaudePaths::for_project(root)
        .map_err(|e| Refusal::Io { detail: e.to_string() })?
        .for_spec(spec.trim())
        .map_err(|_| Refusal::BadSpecName { spec: spec.to_string() })?;
    Ok(paths.spec_html_path())
}

/// A lista dos itens sem dono gravada, e o que a conferência antes de
/// publicar fez nela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnersPage {
    /// Onde ela foi gravada, relativo ao projeto: ao lado da página da spec.
    pub html: String,
    pub withheld: Vec<String>,
    pub warnings: Vec<String>,
}

/// Grava a lista dos itens sem dono da spec `spec` (`owners.html`, ao lado da
/// página da spec), conferida para publicar como as outras páginas. O arquivo
/// de eventos não muda.
pub(crate) fn write_owners(
    root: &Path,
    spec: &str,
    log: &SpecLog,
    lines: &[OwnerLine],
    lang: Locale,
) -> Result<OwnersPage, Refusal> {
    let path = spec_html_path(root, spec)?.with_file_name(OWNERS_PAGE);
    let checked = publishable(owners_page(spec.trim(), log, lines, lang), lang, PAGE_MAX_BYTES);
    write(&path, &checked.html)?;
    Ok(OwnersPage { html: relative(root, &path), warnings: warnings(&checked, lang), withheld: checked.withheld })
}

fn write(path: &Path, text: &str) -> Result<(), Refusal> {
    mustard_core::io::fs::write_atomic(path, text.as_bytes()).map_err(|e| Refusal::Io { detail: e.to_string() })
}

/// A página `doc` pronta para publicar: com cada trecho com cara de segredo
/// trocado por "…" e, se passaria de `max` bytes, sem os registros mais
/// antigos da conversa que forem precisos para caber.
fn publishable(mut doc: Document, lang: Locale, max: usize) -> Publishable {
    let (withheld, loose) = doc.redact(&secret::secret_excerpts, mustard_core::domain::spec_events::PURGED_MARK);
    let html = Render::Html.render(&doc);
    if html.len() <= max {
        return Publishable { html, withheld, loose, trimmed: 0, too_big: false };
    }
    // Cortar um registro a mais nunca deixa a página maior: a menor quantidade
    // que cabe é achada pela metade do intervalo a cada volta.
    let cut = |count: usize| {
        let mut shorter = doc.clone();
        let trimmed = cut_oldest_conversation(&mut shorter, count, lang);
        (Render::Html.render(&shorter), trimmed)
    };
    let (mut low, mut high) = (1, conversation_len(&doc));
    let (mut best, mut trimmed) = cut(high);
    if high == 0 || best.len() > max {
        let too_big = best.len() > max;
        return Publishable { html: best, withheld, loose, trimmed, too_big };
    }
    while low < high {
        let mid = low + (high - low) / 2;
        let (html, count) = cut(mid);
        if html.len() <= max {
            (best, trimmed, high) = (html, count, mid);
        } else {
            low = mid + 1;
        }
    }
    Publishable { html: best, withheld, loose, trimmed, too_big: false }
}

/// O que a conferência antes de publicar precisa dizer.
fn warnings(checked: &Publishable, lang: Locale) -> Vec<String> {
    let mut out = Vec::new();
    let count = checked.withheld.len() + checked.loose;
    if count > 0 {
        let mut places = checked.withheld.clone();
        if checked.loose > 0 {
            places.push(format!("+{}", checked.loose));
        }
        out.push(
            translate("page.withheld_found", lang)
                .replace("{count}", &count.to_string())
                .replace("{codes}", &places.join(", ")),
        );
    }
    if checked.trimmed > 0 {
        out.push(translate("page.conversation.cut", lang).replace("{count}", &checked.trimmed.to_string()));
    }
    if checked.too_big {
        out.push(
            translate("page.too_big", lang)
                .replace("{bytes}", &checked.html.len().to_string())
                .replace("{max}", &PAGE_MAX_BYTES.to_string()),
        );
    }
    out
}

/// Um aviso a mais na lista `warnings` da resposta de um passo, que nasce
/// quando falta.
pub(crate) fn push_warning(report: &mut Value, reason: &str, hint: &str) {
    let warning = json!({ "reason": reason, "hint": hint });
    match report.get_mut("warnings").and_then(Value::as_array_mut) {
        Some(list) => list.push(warning),
        None => report["warnings"] = json!([warning]),
    }
}

/// O item que guarda um trecho com cara de segredo, na resposta de um passo
/// da spec `spec`: o código de cada um em `withheld` e, em `warnings`, a
/// ordem de expurgá-lo. Sem item assim, a resposta não muda.
pub(crate) fn note_withheld(report: &mut Value, spec: &str, withheld: &[String], lang: Locale) {
    if withheld.is_empty() {
        return;
    }
    report["withheld"] = json!(withheld);
    push_warning(report, "page-check", &purge_pending(spec, withheld, lang));
}

/// A ordem de expurgar os itens `withheld` da spec `spec`.
fn purge_pending(spec: &str, withheld: &[String], lang: Locale) -> String {
    translate("page.purge_pending", lang).replace("{codes}", &withheld.join(", ")).replace("{spec}", spec.trim())
}

/// O fim de um passo que é um marco (`approval`, `round` ou `close`) da spec
/// `spec`, com a cópia preparada: a resposta diz em `copy` os lotes de cada
/// página, em `publish` as páginas que ainda precisam da primeira publicação,
/// e manda, em `next`, publicar cada uma delas, copiar os lotes, gravar cada
/// cópia feita e expurgar o item que guarda um trecho com cara de segredo, e
/// segue com `then`. Quando a cópia não pôde ser preparada, o motivo vai
/// para os avisos e a resposta não manda copiar nada.
pub(crate) fn end_milestone(
    report: &mut Value,
    prepared: Result<&copy::Prepared, &Refusal>,
    spec: &str,
    milestone: &str,
    then: &str,
    lang: Locale,
) {
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(refusal) => {
            push_warning(report, refusal.reason(), &refusal.message(lang));
            report["next"] = json!(format!("{} {then}", translate("page.copy.failed", lang)));
            return;
        }
    };
    note_withheld(report, spec, &prepared.withheld, lang);
    let publish = prepared.to_publish();
    if !publish.is_empty() {
        report["publish"] = json!(publish);
    }
    report["copy"] = prepared.to_value();
    let mut next = prepared.order(spec.trim(), Some(milestone), lang);
    // O template que nasce num marco que não é a aprovação é o de uma spec
    // aprovada por uma versão antiga, que não pedia nota: as tarefas das
    // ondas que ainda não saíram ganham a nota, e a onda acima do teto volta
    // para o usuário. Na aprovação, a conferência do plano já cobra as duas.
    if let Some(points) = prepared.points.as_ref().filter(|p| milestone != "approval" && !p.is_clear()) {
        let over: Vec<Value> = points.over_cap().iter().map(|(wave, sum)| json!({ "wave": wave, "points": sum })).collect();
        report["migration"] = json!({ "unrated": points.unrated, "over_cap": over });
        next.extend(migration_order(points, lang));
    }
    if !prepared.withheld.is_empty() {
        next.push(purge_pending(spec, &prepared.withheld, lang));
    }
    report["next"] = json!(numbered_next(next, then));
}

/// Junta as ordens de `orders` com `then` num texto só. Mais de uma ordem sai
/// numa lista numerada, uma por linha, na ordem de fazer, com `then` depois
/// dela, na linha seguinte e sem número, para a rodada continuar separando-o
/// do resto. Com uma ordem só, ou nenhuma, o texto sai como antes: as partes
/// juntas com espaço, num parágrafo só.
fn numbered_next(orders: Vec<String>, then: &str) -> String {
    if orders.len() > 1 {
        let list = orders.iter().enumerate().map(|(i, order)| format!("{}. {order}", i + 1)).collect::<Vec<_>>().join("\n");
        format!("{list}\n{then}")
    } else {
        let mut parts = orders;
        parts.push(then.to_string());
        parts.join(" ")
    }
}

/// A ordem da migração das notas: dar nota às tarefas `points.unrated` e
/// levar ao usuário cada onda acima do teto.
fn migration_order(points: &crate::commands::flow::plan::WavePoints, lang: Locale) -> Vec<String> {
    let cap = crate::commands::flow::plan::WAVE_POINTS_CAP.to_string();
    let mut out = Vec::new();
    if !points.unrated.is_empty() {
        out.push(
            translate("page.migration.unrated", lang)
                .replace("{tasks}", &points.unrated.join(", "))
                .replace("{scale}", translate("plan.points_scale", lang))
                .replace("{cap}", &cap),
        );
    }
    for (wave, sum) in points.over_cap() {
        out.push(
            translate("page.migration.over_cap", lang)
                .replace("{wave}", &wave.to_string())
                .replace("{points}", &sum.to_string())
                .replace("{cap}", &cap),
        );
    }
    out
}

/// O caminho relativo ao projeto, com barras normais: a saída não traz o
/// caminho da máquina.
pub(crate) fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::spec_events as store;
    use mustard_core::view::document::{Group, Item, Node, Section};
    use tempfile::tempdir;

    /// Os dois sinais de uma spec do formato antigo: a pasta só com o
    /// `meta.json`, e o `spec.md` com a seção de critérios de aceitação, mesmo
    /// com arquivo de eventos ao lado. `run write` já recusa toda gravação
    /// numa spec assim
    /// (`run_write_refuses_the_binary_author_and_every_write_to_an_old_format_spec`,
    /// em `spec_events/write.rs`).
    #[test]
    fn the_two_signals_of_an_old_format_spec_are_recognized() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let text = "# Rascunho\n\nO texto da spec.\n";
        let with_criteria = "# Rascunho\n\n## Acceptance Criteria\n\n- [ ] AC-1: passa — Command: `cd .`\n";
        for (spec, md, with_events) in
            [("so-meta", text, false), ("com-criterios", with_criteria, true)]
        {
            let spec_dir = root.join(".claude").join("spec").join(spec);
            std::fs::create_dir_all(&spec_dir).unwrap();
            std::fs::write(spec_dir.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();
            std::fs::write(spec_dir.join("spec.md"), md).unwrap();
            if with_events {
                let plan = serde_json::json!({ "phase": "plan" });
                store::write(&spec_dir.join("spec.ndjson"), "state", plan.as_object().cloned().unwrap(), &[])
                    .unwrap();
            }
            assert!(old_format_spec(root, spec), "{spec}");
        }
    }

    /// Uma spec com arquivo de eventos nunca é do formato antigo por um
    /// `meta.json` que apareceu ao lado.
    #[test]
    fn a_spec_with_an_event_file_is_never_old_format_even_with_a_meta_json() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec_dir = root.join(".claude").join("spec").join("nova");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let survey = serde_json::json!({ "phase": "survey" });
        store::write(&spec_dir.join("spec.ndjson"), "state", survey.as_object().cloned().unwrap(), &[]).unwrap();
        std::fs::write(spec_dir.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();

        assert!(!old_format_spec(root, "nova"));
    }

    fn put(root: &Path, spec: &str, event_type: &str, draft: serde_json::Value) -> u64 {
        let path = root.join(".claude").join("spec").join(spec).join("spec.ndjson");
        store::write(&path, event_type, draft.as_object().cloned().unwrap(), &[]).unwrap().id
    }

    fn read(root: &Path, relative: &str) -> String {
        std::fs::read_to_string(root.join(relative)).unwrap()
    }

    /// Um item de teste, sem título nem campo, só com o código e o texto que a
    /// conferência antes de publicar vai examinar.
    fn item(code: &str, text: &str) -> Node {
        Node::Item(Item {
            code: code.to_string(),
            anchored: true,
            title: String::new(),
            status: None,
            who: None,
            mark: None,
            date: None,
            text: text.to_string(),
            fields: Vec::new(),
        })
    }

    /// Uma página de teste com uma seção "conversation" só, com um grupo
    /// único (um "dia"), para exercitar [`publishable`] e o corte da
    /// conversa sem passar pelo motor antigo (`spec_page`), que saiu.
    fn doc_with(items: Vec<Node>) -> Document {
        Document {
            lang: "pt-BR".into(),
            kind: None,
            title: "t".into(),
            meta: Vec::new(),
            footer: None,
            body: vec![Node::Section(Section {
                anchor: Some("conversation".into()),
                heading: "Conversa".into(),
                body: vec![Node::Group(Group {
                    anchor: "conversation-1".into(),
                    title: "Dia".into(),
                    status: None,
                    summary: String::new(),
                    open: false,
                    body: items,
                })],
            })],
        }
    }

    /// A página que passaria de 16 MB perde os itens mais antigos da
    /// conversa, só os precisos para caber; uma a menos não bastaria.
    #[test]
    fn publishable_drops_the_oldest_conversation_entries_to_fit_the_byte_cap() {
        let megabyte = |n: usize| format!("mensagem {n:02} {}", "palavra ".repeat(125_000));
        let items: Vec<Node> = (1..=18).map(|n| item(&format!("MSTD-MSG-{n:04}"), &megabyte(n))).collect();
        let doc = doc_with(items);

        let checked = publishable(doc.clone(), Locale::PtBr, PAGE_MAX_BYTES);
        assert!(checked.html.len() <= PAGE_MAX_BYTES, "the page has {} bytes", checked.html.len());
        assert!(checked.trimmed > 0, "{:?}", checked.trimmed);
        for n in 1..=checked.trimmed {
            assert!(!checked.html.contains(&format!("mensagem {n:02} ")), "message {n} is still on the page");
        }
        let kept = checked.trimmed + 1;
        assert!(checked.html.contains(&format!("mensagem {kept:02} ")), "message {kept} was cut without need");

        // Com um item a menos cortado, a página não cabia.
        let mut fewer = doc;
        cut_oldest_conversation(&mut fewer, checked.trimmed - 1, Locale::PtBr);
        assert!(Render::Html.render(&fewer).len() > PAGE_MAX_BYTES, "one entry fewer would not fit");
    }

    /// Uma página pequena sai inteira, sem cortar nada.
    #[test]
    fn a_small_page_keeps_the_whole_conversation() {
        let checked = publishable(doc_with(vec![item("MSTD-MSG-0001", "oi")]), Locale::PtBr, PAGE_MAX_BYTES);
        assert_eq!((checked.trimmed, checked.withheld.len()), (0, 0));
        assert!(checked.html.contains("oi") && !checked.html.contains("ficaram só no"));
    }

    /// A publicação da página do projeto, gravada pelo `run write` na spec em
    /// que o passo corre, leva o endereço para a linha do projeto do índice,
    /// de onde a barra de status o lê; o link da página da spec não muda.
    #[test]
    fn the_project_page_address_is_recorded_on_the_project_line() {
        use crate::commands::spec_events::write::{write_at, WriteOpts};

        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        put(root, "busca", "state", serde_json::json!({"phase": "plan"}));
        let publish = |page: &str, url: &str| {
            write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("busca".into()),
                event_type: "publish".into(),
                json: serde_json::json!({"page": page, "milestone": "approval", "ok": true, "url": url}).to_string(),
            })
        };
        let spec = publish("spec", "https://claude.ai/code/artifact/busca");
        assert_eq!(spec["ok"], serde_json::json!(true), "{spec}");
        let project = publish("project", "https://claude.ai/code/artifact/projeto");
        assert_eq!(project["ok"], serde_json::json!(true), "{project}");

        let index = read(root, ".claude/spec/index.ndjson");
        assert_eq!(
            mustard_core::domain::spec_index::project_url(&index).as_deref(),
            Some("https://claude.ai/code/artifact/projeto"),
            "{index}"
        );
        let rows = mustard_core::io::spec_index::read_rows(root);
        assert_eq!(rows[0].url.as_deref(), Some("https://claude.ai/code/artifact/busca"), "{rows:?}");
    }

    /// Cada forma comum de escrever um segredo sai da página como "…", com o
    /// resto do texto legível e o código do item em `withheld`; o que tem
    /// letra e número sem ser segredo — código de item, data, caminho com
    /// linha, leitura de variável de ambiente — continua na página. A mesma
    /// proteção que a lista dos itens sem dono já exercita
    /// (`page_owners_lists_each_unowned_item_with_its_owner_and_where_it_came_from`,
    /// em `spec/page.rs`), aqui pelas formas de segredo, não pelo comando.
    #[test]
    fn publishable_withholds_every_common_secret_form_and_keeps_the_rest() {
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
        let mut codes = Vec::new();
        let mut items = Vec::new();
        for (n, secret) in secrets.iter().enumerate() {
            let code = format!("MSTD-MSG-{:04}", n + 1);
            items.push(item(&code, &format!("o valor é {secret} e pronto")));
            codes.push(code);
        }
        for (n, text) in ordinary.iter().enumerate() {
            items.push(item(&format!("MSTD-MSG-{:04}", secrets.len() + n + 1), text));
        }

        let checked = publishable(doc_with(items), Locale::PtBr, PAGE_MAX_BYTES);
        assert_eq!(checked.withheld, codes, "{:?}", checked.withheld);
        for value in ["S3nh4F0rte2024", "a1b2c3d4e5f6g7h8i9j0", "bPxRfiCYEXAMPLEKEY", "Hunt3rDois", "9f8e7d6c5b4a3210",
            "Banc0Loja", "8f14e45fceea167a5a36dedd4bea2543", &project_key, &npm, "Pr0dSenha", "Ch4veDoSite",
            "Ch4veDoJwt", "Ch4veDoAzure", "Ch4veDaFrase", "senhadobanco", "senhadocache"]
        {
            assert!(!checked.html.contains(value), "{value} reached the page");
        }
        for text in ["MSTD-TASK-0101", "2026-09-17T02:35:53-03:00", "rtk_gain.rs:120", "process.env.GITHUB_TOKEN2",
            "MSTD-DEC-0138", "process.env.SECRET_KEY2", "postgres://usuário:senha@host", "REDIS_PASSWORD"]
        {
            assert!(checked.html.contains(text), "{text} was withheld without being a secret");
        }
        assert!(checked.html.contains("o valor é DB_PASSWORD=… e pronto"), "only the excerpt leaves the page");
    }

    /// Mais de uma ordem sai numa lista numerada, uma por linha, na ordem de
    /// fazer, com `then` depois dela, na linha seguinte e sem número. Com uma
    /// ordem só, ou nenhuma, o texto sai como antes, tudo junto com espaço.
    #[test]
    fn the_milestone_orders_come_as_a_numbered_list() {
        let orders = vec!["Publique a página da spec.".to_string(), "Publique a página do projeto.".to_string()];
        assert_eq!(
            numbered_next(orders, "Copie os lotes."),
            "1. Publique a página da spec.\n2. Publique a página do projeto.\nCopie os lotes."
        );
        assert_eq!(numbered_next(vec!["Publique a página da spec.".to_string()], "Copie os lotes."), "Publique a página da spec. Copie os lotes.");
        assert_eq!(numbered_next(Vec::new(), "Copie os lotes."), "Copie os lotes.");
    }
}

