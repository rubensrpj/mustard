//! `mustard-rt run spend` — o gasto de cada dia, contado pelas conversas e
//! mostrado numa página.
//!
//! Sem argumento, o comando conta os dias fechados que faltam (do dia seguinte
//! ao último contado até ontem), guarda as linhas no arquivo do gasto da
//! máquina e conta hoje, o dia aberto, de novo a cada pedido: as linhas de
//! hoje vão à página como parciais e nunca ao arquivo dos dias fechados. Em
//! seguida prepara a cópia para o banco da página: o template, os lotes (as
//! linhas fechadas que faltam, as de hoje e o resumo da máquina) e a ordem do
//! que o orquestrador faz. A cópia preparada vale como feita: o arquivo do
//! gasto já guarda até que dia a página recebeu e a versão de cada documento
//! que a cópia seguinte troca. Funciona em qualquer projeto, com ou sem spec.
//!
//! - `--republish` prepara a publicação nova e a cópia de todos os dias: o
//!   caminho de quem perdeu o link da página ou teve um lote que falhou.
//! - `--url <endereço>` grava o endereço que a publicação devolveu.
//!
//! A regra mora em `mustard_core::domain::spend` e `mustard_core::io::spend`.
//! Recusa sai com exit 1, `ok: false`, a razão em `reason` e a mensagem no
//! idioma do projeto em `hint`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mustard_core::domain::spend::{summarize, DayRow, Ledger, Refusal};
use mustard_core::io::spend as store;
use mustard_core::platform::harness::claude_config_dir;
use mustard_core::platform::i18n::Locale;
use serde_json::{json, Value};

use crate::commands::spec_events::pages::spend as copy;
use crate::commands::spec_events::project;

/// Options for `mustard-rt run spend`.
pub struct SpendOpts {
    /// Qualquer pasta dentro do repositório: só dá o idioma das mensagens.
    pub root: PathBuf,
    /// Prepara a publicação nova e a cópia de todos os dias.
    pub republish: bool,
    /// O endereço que a publicação devolveu, a gravar.
    pub url: Option<String>,
}

/// A máquina onde o comando roda: a pasta do gasto, a pasta de configuração
/// do Claude Code, onde moram as conversas, e o dia de hoje. O comando real
/// os lê do ambiente; o teste os dá por argumento.
pub(crate) struct Machine {
    pub dir: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub today: String,
}

impl Machine {
    /// A máquina de hoje, como o ambiente a diz.
    pub(crate) fn here() -> Self {
        Self { dir: store::machine_dir(), config: claude_config_dir(), today: store::today() }
    }
}

/// O núcleo testável de [`run`]: a resposta do comando ou a recusa.
pub(crate) fn spend_at(opts: &SpendOpts, machine: &Machine) -> Value {
    let lang = project(&opts.root).lang;
    match answer(opts, machine, lang) {
        Ok(report) => report,
        Err(refusal) => json!({ "ok": false, "reason": refusal.reason(), "hint": refusal.message(lang) }),
    }
}

/// O que o pedido faz: grava o endereço, ou conta o que falta, conta hoje e
/// prepara a cópia.
fn answer(opts: &SpendOpts, machine: &Machine, lang: Locale) -> Result<Value, Refusal> {
    let dir = machine.dir.as_deref().ok_or(Refusal::NoMachineFolder)?;
    if let Some(url) = &opts.url {
        return record_url(dir, url);
    }
    let config = machine.config.as_deref().ok_or(Refusal::NoMachineFolder)?;
    let counted = count_missing(dir, config, &machine.today)?;
    let open = store::count_open(config, &machine.today);
    let (prepared, url) =
        store::update(dir, |ledger| prepare(dir, ledger, &open, &machine.today, opts.republish, lang))??;
    let order = copy::order(&prepared, url.as_deref(), opts.republish, lang);
    let published = url.is_some() && !opts.republish;
    Ok(json!({
        "ok": true,
        "counted": counted,
        "copy": { copy::KEY: copy::to_value(&prepared, published) },
        "order": order,
    }))
}

/// Conta os dias fechados que faltam e guarda as linhas no arquivo da
/// máquina; devolve a faixa contada e quantas linhas ela deu, ou `null` quando
/// não faltava dia nenhum. Um dia já contado não se abre de novo.
fn count_missing(dir: &Path, config: &Path, today: &str) -> Result<Value, Refusal> {
    let Some(range) = store::load(dir)?.to_count(today) else { return Ok(Value::Null) };
    let rows = store::count(config, &range);
    let counted = rows.len();
    store::update(dir, |ledger| ledger.record_counted(&range, rows))?;
    Ok(json!({ "first": range.first, "last": range.last, "rows": counted }))
}

/// Prepara a cópia com a trava do arquivo do gasto presa: as linhas fechadas
/// que a página ainda não recebeu, as de hoje e o resumo da máquina, e dá a
/// cópia por feita no arquivo. Uma página nova — a que ainda não tem endereço
/// e a do `--republish` — tem o banco vazio: leva todas as linhas fechadas,
/// sem versão em nenhuma escrita, e o que se sabia do banco anterior não vale.
/// Devolve o que a preparação deixou e o endereço da página, quando ela já
/// tem.
fn prepare(
    dir: &Path,
    ledger: &mut Ledger,
    open: &[DayRow],
    today: &str,
    republish: bool,
    lang: Locale,
) -> Result<(copy::Prepared, Option<String>), Refusal> {
    let summary = summarize(&ledger.rows, open, today);
    let fresh = republish || ledger.url.is_none();
    let empty = BTreeMap::new();
    let prepared = {
        let closed: Vec<&DayRow> = if fresh { ledger.rows.iter().collect() } else { ledger.uncopied() };
        let send = republish || !closed.is_empty() || !open.is_empty() || ledger.url.is_some();
        let versions = if fresh { &empty } else { &ledger.versions };
        copy::prepare(dir, &copy::Plan { closed: &closed, open, summary: &summary, versions, send }, lang)?
    };
    if !prepared.docs.is_empty() {
        ledger.record_sent(prepared.through.clone(), prepared.docs.clone(), fresh);
    }
    Ok((prepared, ledger.url.clone()))
}

/// Grava o endereço da página.
fn record_url(dir: &Path, url: &str) -> Result<Value, Refusal> {
    let url = url.trim();
    if !url.starts_with("https://") || url.len() <= "https://".len() {
        return Err(Refusal::NotAnAddress { found: url.to_string() });
    }
    store::update(dir, |ledger| ledger.url = Some(url.to_string()))?;
    Ok(json!({ "ok": true, "recorded": "url" }))
}

/// CLI entry — `mustard-rt run spend`.
pub fn run(opts: &SpendOpts) {
    let report = spend_at(opts, &Machine::here());
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string()));
    let _ = std::io::Write::flush(&mut std::io::stdout());
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::platform::i18n::translate;
    use std::fs;

    /// Uma máquina de mentira: a pasta do gasto e a da configuração do Claude
    /// Code dentro de `base`, e hoje em `today`.
    fn machine(base: &Path, today: &str) -> Machine {
        Machine { dir: Some(base.join("spend")), config: Some(base.join("config")), today: today.to_string() }
    }

    fn opts(root: &Path) -> SpendOpts {
        SpendOpts { root: root.to_path_buf(), republish: false, url: None }
    }

    /// Um projeto com `mustard.json` na pasta `name` de `base`, no idioma
    /// `text`.
    fn project(base: &Path, name: &str, text: &str) -> PathBuf {
        let root = base.join(name);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{text}"}}}}"#)).unwrap();
        root
    }

    /// Uma conversa do projeto `cwd` com uma resposta de 10 tokens em cada dia
    /// de `days`.
    fn conversation(base: &Path, cwd: &Path, days: &[&str]) {
        let dir = base.join("config/projects/p");
        fs::create_dir_all(&dir).unwrap();
        let lines: Vec<String> = days
            .iter()
            .enumerate()
            .map(|(n, day)| {
                json!({"timestamp": format!("{day}T15:00:00Z"), "cwd": cwd.to_string_lossy(), "message": {
                    "id": format!("m{n}"), "usage": {"input_tokens": 10, "output_tokens": 0}, "content": []}})
                .to_string()
            })
            .collect();
        fs::write(dir.join("s1.jsonl"), lines.join("\n")).unwrap();
    }

    /// O nome de cada documento da cópia preparada em `answer`, na ordem dos
    /// lotes, e a versão com que ele sai.
    fn docs_of(answer: &Value) -> Vec<(String, Value)> {
        let batches = answer["copy"]["spend"]["writes"].as_array().unwrap();
        let writes = batches.iter().flat_map(|batch| batch.as_array().unwrap());
        writes.map(|write| (write["doc_id"].as_str().unwrap().to_string(), write["if_version"].clone())).collect()
    }

    /// Enquanto a página não tem endereço (a publicação falhou), cada pedido
    /// leva tudo de novo, sem versão, porque o banco dela está vazio; com o
    /// endereço gravado, o pedido seguinte leva só hoje e o resumo, trocando a
    /// versão que o banco tem, e o dia fechado não é contado duas vezes.
    #[test]
    fn a_page_without_an_address_gets_everything_again_and_a_published_one_only_today_and_the_summary() {
        let dir = tempfile::tempdir().unwrap();
        let machine = machine(dir.path(), "2026-10-02");
        let root = project(dir.path(), "loja", "pt-BR");
        conversation(dir.path(), &root, &["2026-09-30", "2026-10-01", "2026-10-02"]);

        let first = spend_at(&opts(&root), &machine);
        assert_eq!(first["counted"]["rows"], json!(2), "only the closed days are counted: {first}");
        let names = ["2026-09-30-loja", "2026-10-01-loja", "2026-10-02-loja", "current"];
        let docs = docs_of(&first);
        assert_eq!(docs.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), names, "the closed days, the open day, the summary");
        assert!(docs.iter().all(|(_, version)| version.is_null()), "a page with no address has an empty database");

        let again = spend_at(&opts(&root), &machine);
        assert_eq!(docs_of(&again), docs, "with no address every line goes again: {again}");
        assert_eq!(again["counted"], Value::Null, "a closed day is not counted twice");
        assert!(again["order"][0].as_str().unwrap().contains("page.html"), "the page is published: {again}");

        let url = Some("https://claude.ai/code/artifact/a".to_string());
        assert_eq!(spend_at(&SpendOpts { url, ..opts(&root) }, &machine)["ok"], json!(true));
        let next = docs_of(&spend_at(&opts(&root), &machine));
        assert_eq!(next, [("2026-10-02-loja".to_string(), json!(1)), ("current".to_string(), json!(1))]);
    }

    /// A publicação nova leva todos os dias fechados mais hoje, sem versão em
    /// nenhuma escrita, porque o banco da página nova está vazio, e manda
    /// publicar a página de novo; a cópia seguinte volta a levar só hoje e o
    /// resumo, com a versão do banco novo.
    #[test]
    fn republishing_sends_every_day_to_an_empty_database() {
        let dir = tempfile::tempdir().unwrap();
        let machine = machine(dir.path(), "2026-10-02");
        let root = project(dir.path(), "loja", "pt-BR");
        conversation(dir.path(), &root, &["2026-09-29", "2026-09-30", "2026-10-01", "2026-10-02"]);
        let record = |url: &str| spend_at(&SpendOpts { url: Some(url.into()), ..opts(&root) }, &machine);
        spend_at(&opts(&root), &machine);
        record("https://claude.ai/code/artifact/a");
        assert_eq!(docs_of(&spend_at(&opts(&root), &machine)).len(), 2, "with the page published only today and the summary go");

        let all = spend_at(&SpendOpts { republish: true, ..opts(&root) }, &machine);
        let docs = docs_of(&all);
        let names: Vec<&str> = docs.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["2026-09-29-loja", "2026-09-30-loja", "2026-10-01-loja", "2026-10-02-loja", "current"]);
        assert!(docs.iter().all(|(_, version)| version.is_null()), "a new page has an empty database: {all}");
        assert!(all["order"][0].as_str().unwrap().contains("page.html"), "the first step publishes the page again: {all}");

        record("https://claude.ai/code/artifact/b");
        let next = docs_of(&spend_at(&opts(&root), &machine));
        assert_eq!(next, [("2026-10-02-loja".to_string(), json!(1)), ("current".to_string(), json!(1))], "the new database starts at version 1");
    }

    /// O endereço que não começa por `https://` é recusado nos dois idiomas,
    /// sem gravar nada; sem a pasta pessoal para guardar o gasto o comando
    /// recusa e não escreve nada; e um arquivo do gasto que não se lê é
    /// recusado sem ser tocado, com o conserto na mensagem: apagar o arquivo.
    #[test]
    fn the_command_refuses_in_the_project_language_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let machine = machine(dir.path(), "2026-10-02");
        for (text, lang) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
            let root = project(dir.path(), text, text);
            let refused = spend_at(&SpendOpts { url: Some("claude.ai/x".into()), ..opts(&root) }, &machine);
            assert_eq!((refused["ok"].clone(), refused["reason"].clone()), (json!(false), json!("not-an-address")), "{text}");
            let expected = translate("page.spend.refusal.not_an_address", lang).replace("{found}", "claude.ai/x");
            assert_eq!(refused["hint"], json!(expected), "{text}: the hint speaks the project language");
            let no_folder = spend_at(&opts(&root), &Machine { dir: None, config: None, today: "2026-10-02".into() });
            assert_eq!(no_folder["reason"], json!("no-machine-folder"));
            assert_eq!(no_folder["hint"], json!(translate("page.spend.refusal.no_machine_folder", lang)), "{text}");
        }
        assert!(!machine.dir.as_ref().unwrap().exists(), "a refusal writes nothing");

        let root = project(dir.path(), "loja", "en-US");
        let folder = machine.dir.clone().unwrap();
        fs::create_dir_all(&folder).unwrap();
        fs::write(store::ledger_path(&folder), "{ not json").unwrap();
        let refused = spend_at(&opts(&root), &machine);
        assert_eq!(refused["reason"], json!("unreadable-ledger"), "{refused}");
        assert!(refused["hint"].as_str().unwrap().contains("Delete the file"), "{refused}");
        assert_eq!(fs::read_to_string(store::ledger_path(&folder)).unwrap(), "{ not json");
    }
}
