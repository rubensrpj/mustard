//! A linha de cada spec entregue no resumo do projeto e o arquivo de regras
//! dela, gravados pelo programa a partir do texto que a spec já tem.
//!
//! A entrega grava só a linha, sem regra e sem chamar o Jev, e deixa as
//! regras da spec pendentes ([`record_delivered`]). A decisão das regras, as
//! specs antigas e as falhas do projeto ficam com a passagem das pendências
//! ([`crate::shared::project_summary::run_pending`]), que grava a linha por
//! aqui ([`write_entry`]).
//!
//! O módulo só depende do núcleo e dos recibos de custo, que também só
//! dependem dele: a gravação de eventos o chama na entrega, e nada daqui
//! chega à rodada, ao painel, à retomada nem ao fechamento.

use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::domain::project_summary::{self as summary, Agreed, Entry};
use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::project_summary as disk;
use mustard_core::io::spec_events as store;
use serde_json::json;

/// Grava as regras da spec `spec` e a linha dela no resumo do projeto `root`,
/// a partir do arquivo de eventos `log`, com a chance de sim de cada item em
/// `chances` e o custo conhecido em `costs`.
pub(crate) fn write_entry(root: &Path, spec: &str, log: &SpecLog, items: &[Agreed], chances: &BTreeMap<u64, f64>, costs: &BTreeMap<String, u64>) -> mustard_core::platform::error::Result<Entry> {
    let lang = mustard_core::ProjectConfig::load(root).language().text_or_default();
    let groups = summary::group_rules(items, chances);
    let rules = disk::write_rules(root, spec, &groups, lang)?;
    let (title, delivered) = summary::title_and_delivered(log, spec, lang);
    let entry = Entry {
        spec: spec.to_string(),
        delivered_at: summary::delivered_at(log),
        pr: summary::pr_of(log),
        title,
        delivered,
        rules,
        skills: summary::skills_of(log),
        cost: costs.get(spec).map(|micro| json!({ "micro_usd": micro, "basis": "host-api-equivalent-estimate" })),
    };
    disk::upsert(root, &entry)?;
    Ok(entry)
}

/// Grava o que a spec `spec`, vista de `start`, deixa ao projeto na entrega:
/// a linha dela no resumo, sem regra — as que ela deixou antes saem —, e a
/// marca de que as regras dela esperam a decisão do Jev. Não chama o Jev.
///
/// # Errors
///
/// A spec que não se lê e a falha de gravação, com o motivo; quem chama avisa
/// e a entrega segue.
pub(crate) fn record_delivered(start: &Path, spec: &str) -> Result<(), String> {
    let root = store::spec_root(start);
    let log = store::spec_file(&root, spec).ok().and_then(|file| store::read(&file).ok().flatten()).ok_or_else(|| format!("spec-unreadable: {spec}"))?;
    let items = summary::agreed_items(&log);
    write_entry(&root, spec, &log, &items, &BTreeMap::new(), &crate::shared::session_costs::by_spec(&root)).map_err(|error| error.to_string())?;
    disk::add_pending_rules(&root, spec).map_err(|error| error.to_string())
}
