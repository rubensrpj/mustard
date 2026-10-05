//! A cópia do gasto para o banco de dados da página do gasto.
//!
//! A página é uma só por máquina: o template dela, os documentos e os lotes
//! moram na pasta do gasto da máquina (`io::spend::machine_dir`), não na de um
//! projeto. A preparação segue o molde da cópia da spec ([`super::copy`]): cada
//! documento vai num arquivo JSON próprio e os arquivos entram em lotes de até
//! [`copy::BATCH_MAX`] escritas, que o orquestrador manda ao banco pela
//! ferramenta `ArtifactData`; a resposta traz as escritas de cada lote prontas,
//! em `copy.spend.writes`, com o `file_path` absoluto.
//!
//! A cópia leva as linhas dos dias fechados que a página ainda não recebeu (a
//! página nova, da primeira publicação e do `--republish`, leva todas), as
//! linhas de hoje como parciais, a cada cópia, e o resumo da máquina
//! (`summary/current`). O banco só troca um documento que já existe quando a
//! escrita traz a versão dele (`if_version`): o arquivo do gasto guarda a das
//! linhas do dia aberto e a do resumo, e a preparação as põe em cada escrita.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::Refusal as SpecRefusal;
use mustard_core::domain::spend::{DayRow, Refusal, Summary, SUMMARY_COLLECTION, SUMMARY_DOC};
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::platform::page_templates::{spend_page_template, DAYS, SPEND_CAPABILITIES};
use serde_json::{json, Value};

use super::copy;

/// A chave da página do gasto na resposta (`copy.spend`) e no catálogo.
pub(crate) const KEY: &str = "spend";

/// O template da página do gasto, dentro da pasta da máquina.
const TEMPLATE: &str = "page.html";

/// A pasta, dentro da pasta da máquina, onde os lotes de cada preparação
/// ficam.
const FOLDER: &str = "copy";

/// O nome dos lotes: `days-1.json`, `days-2.json`…
const BATCH_NAME: &str = "days";

/// O que a preparação leva ao banco.
pub(crate) struct Plan<'a> {
    /// As linhas dos dias fechados que a página ainda não recebeu.
    pub closed: &'a [&'a DayRow],
    /// As linhas de hoje, o dia aberto.
    pub open: &'a [DayRow],
    /// O resumo da máquina inteira.
    pub summary: &'a Summary,
    /// A versão de cada documento que o banco já tem, pelo nome
    /// `coleção/doc_id`; vazia no banco novo.
    pub versions: &'a BTreeMap<String, u64>,
    /// Se há o que levar ao banco: linhas, ou uma página que já existe e tem
    /// de mostrar o resumo de hoje.
    pub send: bool,
}

/// O que a preparação deixou para o orquestrador copiar.
pub(crate) struct Prepared {
    /// Quantas linhas de dia (fechadas e de hoje) os lotes levam.
    pub rows: usize,
    /// Os arquivos dos lotes, relativos à pasta da máquina.
    pub batches: Vec<String>,
    /// As escritas de cada lote, com o `file_path` absoluto.
    pub writes: Vec<Vec<Value>>,
    /// O último dia fechado das linhas dos lotes; `None` sem linha fechada.
    pub through: Option<String>,
    /// Os documentos que a cópia seguinte troca (as linhas do dia aberto e o
    /// resumo), pelo nome `coleção/doc_id`.
    pub docs: Vec<String>,
    /// O caminho absoluto do template que a publicação usa.
    pub template: String,
}

/// Prepara em `dir` o template e os lotes do `plan`, apagando a preparação
/// anterior: os lotes dela não valem mais. O template sai no idioma `lang`.
///
/// # Errors
///
/// [`Refusal::Io`] quando o disco falha.
pub(crate) fn prepare(dir: &Path, plan: &Plan<'_>, lang: Locale) -> Result<Prepared, Refusal> {
    copy::ensure_template(dir, TEMPLATE, &spend_page_template(lang)).map_err(io)?;
    let folder = dir.join(FOLDER);
    copy::clear(&folder).map_err(io)?;
    let template = absolute(&dir.join(TEMPLATE)).to_string_lossy().replace('\\', "/");
    if !plan.send {
        return Ok(Prepared { rows: 0, batches: Vec::new(), writes: Vec::new(), through: None, docs: Vec::new(), template });
    }
    let through = plan.closed.iter().map(|row| row.day.as_str()).max().map(str::to_string);
    let mark = mark_of(&plan.summary.today.day);
    let mut writes = Vec::new();
    for row in plan.closed.iter().copied().chain(plan.open) {
        writes.push(copy::set_in(dir, &folder, mark, DAYS, &row.doc_id(), &json!(row)).map_err(io)?);
    }
    writes.push(copy::set_in(dir, &folder, mark, SUMMARY_COLLECTION, SUMMARY_DOC, &json!(plan.summary)).map_err(io)?);
    copy::pin(&mut writes, Vec::new(), |name| plan.versions.get(name).map(|version| json!(version)));
    let docs = writes[plan.closed.len()..].iter().map(copy::doc_name).collect();
    let (batches, sent) = copy::batches_in(dir, &folder, BATCH_NAME, &writes).map_err(io)?;
    Ok(Prepared {
        rows: plan.closed.len() + plan.open.len(),
        batches,
        writes: copy::absolute(dir, sent),
        through,
        docs,
        template,
    })
}

/// A marca de uma preparação: o dia dela como número (`20261002`), no nome de
/// cada documento.
fn mark_of(day: &str) -> u64 {
    day.replace('-', "").parse().unwrap_or_default()
}

/// O caminho como a ferramenta do banco o lê, sem depender da pasta de onde o
/// comando roda.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Toda falha de disco da cópia da spec vira a falha de disco do gasto.
fn io(refusal: SpecRefusal) -> Refusal {
    match refusal {
        SpecRefusal::Io { detail } => Refusal::Io { detail },
        other => Refusal::Io { detail: other.reason().to_string() },
    }
}

/// A resposta do passo para a cópia: as linhas, os lotes e as escritas de
/// cada um. `published` diz se a página já tem endereço e não vai ser
/// publicada de novo.
pub(crate) fn to_value(prepared: &Prepared, published: bool) -> Value {
    json!({ "published": published, "rows": prepared.rows, "batches": prepared.batches, "writes": prepared.writes })
}

/// A ordem da cópia, uma frase por passo: publicar a página quando ela ainda
/// não tem endereço (ou quando o usuário pediu a publicação nova) e gravar o
/// endereço, copiar os lotes pelo endereço guardado e gravar a cópia feita;
/// no fim, não escrever o endereço na resposta. Vazia quando não há o que
/// publicar nem copiar.
pub(crate) fn order(prepared: &Prepared, url: Option<&str>, republish: bool, lang: Locale) -> Vec<String> {
    let page = translate("page.name.spend", lang);
    let publish = republish || (url.is_none() && prepared.rows > 0);
    let mut out = Vec::new();
    if publish {
        let key = if republish { "page.copy.spend_republish" } else { "page.copy.spend_publish" };
        out.push(
            translate(key, lang)
                .replace("{page}", page)
                .replace("{template}", &prepared.template)
                .replace("{capabilities}", SPEND_CAPABILITIES),
        );
    }
    if !prepared.docs.is_empty() {
        let address = match url {
            Some(url) if !republish => url.to_string(),
            _ => translate("page.copy.new_address", lang).to_string(),
        };
        let copy =
            translate("page.copy.batches", lang).replace("{page}", page).replace("{url}", &address).replace("{key}", KEY);
        out.push(format!("{copy} {}", translate("page.copy.spend_record", lang)));
    }
    if !out.is_empty() {
        out.push(translate("page.copy.no_links", lang).to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::spend::summarize;

    fn row(day: String, partial: bool) -> DayRow {
        DayRow { day, project: "Loja Web".to_string(), actions: 1, partial, ..DayRow::default() }
    }

    /// O plano de `closed` e `open` num dia de hoje, com as `versions` dadas.
    fn prepare_for(dir: &Path, closed: &[DayRow], open: &[DayRow], versions: &BTreeMap<String, u64>) -> Prepared {
        let refs: Vec<&DayRow> = closed.iter().collect();
        let summary = summarize(closed, open, "2026-10-02");
        prepare(dir, &Plan { closed: &refs, open, summary: &summary, versions, send: true }, Locale::PtBr).unwrap()
    }

    /// Uma linha por documento, em lotes de até cinquenta escritas, cada uma
    /// com o caminho absoluto de um arquivo que existe e o corpo da linha, e
    /// o resumo no fim. A linha de hoje vai marcada como parcial, o dia
    /// fechado não, e a escrita de um documento que o banco já tem leva a
    /// versão dele em `if_version`; o que o banco ainda não tem vai sem
    /// versão. A preparação seguinte apaga os lotes da anterior, e sem nada a
    /// levar não sobra lote.
    #[test]
    fn a_copy_goes_in_batches_of_fifty_with_the_open_day_partial_and_the_versions_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let closed: Vec<DayRow> =
            (0..120).map(|n| row(format!("2026-{:02}-{:02}", 1 + n / 28, 1 + n % 28), false)).collect();
        let open = [row("2026-10-02".into(), true)];
        let versions: BTreeMap<String, u64> =
            [("summary/current".to_string(), 3), ("days/2026-10-02-loja-web".to_string(), 2)].into();
        let prepared = prepare_for(dir.path(), &closed, &open, &versions);
        assert_eq!(prepared.rows, 121);
        let sizes: Vec<usize> = prepared.writes.iter().map(Vec::len).collect();
        assert_eq!(sizes, [50, 50, 22], "120 closed lines, today and the summary");
        assert_eq!(prepared.batches, ["copy/days-1.json", "copy/days-2.json", "copy/days-3.json"]);
        assert_eq!(prepared.through.as_deref(), Some("2026-05-08"), "the last closed day of the lines");
        assert_eq!(prepared.docs, ["days/2026-10-02-loja-web", "summary/current"], "only what the next copy replaces");

        let writes: Vec<&Value> = prepared.writes.iter().flatten().collect();
        let first = writes[0];
        assert_eq!((first["op"].as_str(), first["collection"].as_str(), first["doc_id"].as_str()), (Some("set"), Some("days"), Some("2026-01-01-loja-web")));
        assert!(first.get("if_version").is_none(), "the database has no such document yet");
        let body = |write: &Value| -> Value {
            let file = write["file_path"].as_str().unwrap();
            assert!(Path::new(file).is_absolute() && file.contains('@'), "{file}");
            serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
        };
        assert_eq!((body(first)["project"].clone(), body(first)["partial"].clone()), (json!("Loja Web"), json!(false)));
        let (today, summary) = (writes[writes.len() - 2], writes[writes.len() - 1]);
        assert_eq!((body(today)["partial"].clone(), today["if_version"].clone()), (json!(true), json!(2)));
        assert_eq!((summary["doc_id"].clone(), summary["if_version"].clone()), (json!("current"), json!(3)));
        let template = std::fs::read_to_string(&prepared.template).unwrap();
        assert!(template.starts_with("<!-- mustard: layout-"), "the template is stamped: {}", &template[..40]);

        let dropped = prepared.writes[2][0]["file_path"].as_str().unwrap().to_string();
        let again = prepare_for(dir.path(), &closed[..2], &[], &BTreeMap::new());
        assert_eq!(again.batches, ["copy/days-1.json"]);
        assert!(!dir.path().join("copy/days-2.json").exists() && !Path::new(&dropped).exists(), "the old batches are gone");
        let summary = summarize(&[], &[], "2026-10-02");
        let plan = Plan { closed: &[], open: &[], summary: &summary, versions: &BTreeMap::new(), send: false };
        let none = prepare(dir.path(), &plan, Locale::PtBr).unwrap();
        assert_eq!((none.rows, none.batches.len(), none.through, none.docs.len()), (0, 0, None, 0));
    }

    fn prepared(rows: usize) -> Prepared {
        Prepared {
            rows,
            batches: Vec::new(),
            writes: Vec::new(),
            through: None,
            docs: if rows > 0 { vec!["summary/current".into()] } else { Vec::new() },
            template: "/maquina/page.html".into(),
        }
    }

    /// A ordem publica a página só quando ela ainda não tem endereço e há o
    /// que mostrar, ou quando o usuário pediu a publicação nova; copia os
    /// lotes no endereço guardado, ou no que a publicação devolver, e diz como
    /// recomeçar se um lote falhar; e fica vazia sem nada a publicar nem a
    /// copiar.
    #[test]
    fn the_order_publishes_only_without_an_address_or_when_asked() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let url = "https://claude.ai/code/artifact/gasto";
            let publish = translate("page.copy.spend_publish", lang).replace("{page}", translate("page.name.spend", lang));
            let start = publish.split("{template}").next().unwrap_or_default();
            let first = order(&prepared(3), None, false, lang);
            assert_eq!(first.len(), 3, "{lang:?}: {first:?}");
            assert!(first[0].starts_with(start) && first[0].contains("/maquina/page.html"), "{lang:?}: {}", first[0]);
            assert!(first[1].contains("copy.spend.writes") && first[1].contains("spend --republish"), "{lang:?}: {}", first[1]);
            assert!(first[1].contains(translate("page.copy.new_address", lang)), "{lang:?}: the address is the one the publication returns");

            let kept = order(&prepared(3), Some(url), false, lang);
            assert_eq!(kept.len(), 2, "{lang:?}: published already, only the copy: {kept:?}");
            assert!(kept[0].contains(url) && kept[0].contains("copy.spend.writes"), "{lang:?}: {}", kept[0]);

            let asked = order(&prepared(3), Some(url), true, lang);
            assert_eq!(asked.len(), 3, "{lang:?}: {asked:?}");
            assert!(asked[0].contains("/maquina/page.html") && !asked[0].contains(url), "{lang:?}: {}", asked[0]);
            assert_ne!(asked[0], first[0], "{lang:?}: the new publication is told apart from the first");
            assert!(!asked[1].contains(url), "{lang:?}: the copy goes to the new address: {}", asked[1]);

            assert_eq!(order(&prepared(0), None, false, lang), Vec::<String>::new(), "{lang:?}: nothing to show");
            assert_eq!(order(&prepared(0), Some(url), false, lang), Vec::<String>::new(), "{lang:?}: nothing to copy");
            assert_eq!(order(&prepared(0), Some(url), true, lang).len(), 2, "{lang:?}: with nothing to copy it only publishes and keeps the links out");
        }
    }
}
