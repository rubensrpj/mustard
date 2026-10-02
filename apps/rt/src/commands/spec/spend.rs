//! `mustard-rt run spend` — o gasto de cada dia, contado pelas conversas e
//! mostrado numa página.
//!
//! O Claude Code grava cada conversa na máquina, com os tokens e cada
//! ferramenta usada. Sem argumento, o comando conta os dias fechados que
//! faltam (do dia seguinte ao último contado até ontem), guarda as linhas no
//! arquivo do gasto da máquina e conta hoje, o dia aberto, de novo a cada
//! pedido: as linhas de hoje vão à página como parciais e nunca ao arquivo
//! dos dias fechados. Em seguida prepara a cópia para o banco de dados da
//! página do gasto: o template, os lotes (as linhas fechadas que faltam, as de
//! hoje e o resumo da máquina) e a ordem do que o orquestrador faz (publicar a
//! página quando ela ainda não tem endereço e copiar os lotes). Um dia
//! fechado é contado uma vez; recontar é apagar o arquivo do gasto e deixar o
//! comando refazê-lo pelas conversas. Funciona em qualquer projeto, com ou
//! sem spec aberta.
//!
//! - `--republish` prepara a publicação nova, em outro endereço, e a cópia de
//!   todos os dias: é o caminho de quem perdeu o link da página.
//! - `--url <endereço>` grava o endereço que a publicação devolveu.
//! - `--copied` grava a cópia preparada como feita.
//!
//! A regra mora no núcleo (`mustard_core::domain::spend` e
//! `mustard_core::io::spend`); aqui ficam os argumentos, o idioma do projeto
//! e a saída. Recusa sai com exit 1, `ok: false`, a razão curta em `reason` e
//! a mensagem no idioma do projeto em `hint`.

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
    /// Grava a cópia preparada como feita.
    pub copied: bool,
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

/// O que o pedido faz: grava o endereço, grava a cópia como feita, ou conta o
/// que falta, conta hoje e prepara a cópia.
fn answer(opts: &SpendOpts, machine: &Machine, lang: Locale) -> Result<Value, Refusal> {
    let dir = machine.dir.as_deref().ok_or(Refusal::NoMachineFolder)?;
    if let Some(url) = &opts.url {
        return record_url(dir, url);
    }
    if opts.copied {
        return record_copied(dir);
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
/// que a página ainda não recebeu (todas no `--republish`), as de hoje e o
/// resumo da máquina. Guarda até onde a preparação foi e quais documentos ela
/// levou, e devolve o que ela deixou e o endereço da página, quando ela já
/// tem. A publicação nova vai para um banco vazio: nenhuma escrita leva
/// versão.
fn prepare(
    dir: &Path,
    ledger: &mut Ledger,
    open: &[DayRow],
    today: &str,
    republish: bool,
    lang: Locale,
) -> Result<(copy::Prepared, Option<String>), Refusal> {
    let summary = summarize(&ledger.rows, open, today);
    let empty = BTreeMap::new();
    let prepared = {
        let closed: Vec<&DayRow> = if republish { ledger.rows.iter().collect() } else { ledger.uncopied() };
        let send = republish || !closed.is_empty() || !open.is_empty() || ledger.url.is_some();
        let versions = if republish { &empty } else { &ledger.versions };
        copy::prepare(dir, &copy::Plan { closed: &closed, open, summary: &summary, versions, send }, lang)?
    };
    ledger.record_prepared(prepared.through.clone(), prepared.docs.clone());
    Ok((prepared, ledger.url.clone()))
}

/// Grava o endereço da página. Outro endereço que o de antes é uma página
/// nova, com o banco vazio: nada do que foi copiado conta mais.
fn record_url(dir: &Path, url: &str) -> Result<Value, Refusal> {
    let url = url.trim();
    if !url.starts_with("https://") || url.len() <= "https://".len() {
        return Err(Refusal::NotAnAddress { found: url.to_string() });
    }
    store::update(dir, |ledger| ledger.record_url(url))?;
    Ok(json!({ "ok": true, "recorded": "url" }))
}

/// Grava a cópia preparada como feita: a página já tem as linhas fechadas até
/// o último dia que a preparação levou, e cada documento dela subiu uma
/// versão.
fn record_copied(dir: &Path) -> Result<Value, Refusal> {
    let recorded = store::update(dir, Ledger::record_copy)?;
    if !recorded {
        return Err(Refusal::NothingPrepared);
    }
    Ok(json!({ "ok": true, "recorded": "copy" }))
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
        SpendOpts { root: root.to_path_buf(), republish: false, url: None, copied: false }
    }

    /// Um projeto com `mustard.json` na pasta `name` de `base`, no idioma
    /// `text`.
    fn project(base: &Path, name: &str, text: &str) -> PathBuf {
        let root = base.join(name);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{text}"}}}}"#)).unwrap();
        root
    }

    /// Uma conversa com uma resposta de `tokens` em cada dia de `days`, no
    /// projeto `cwd`.
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

    /// O endereço que não começa por `https://` é recusado nos dois idiomas,
    /// sem gravar nada; o certo é guardado, e outro endereço depois dele zera
    /// o que já foi copiado, porque a página nova nasce com o banco vazio.
    #[test]
    fn an_address_is_recorded_and_a_new_one_empties_the_copied_mark() {
        let dir = tempfile::tempdir().unwrap();
        let machine = machine(dir.path(), "2026-10-02");
        for (text, lang) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
            let root = project(dir.path(), text, text);
            let refused = spend_at(&SpendOpts { url: Some("claude.ai/x".into()), ..opts(&root) }, &machine);
            assert_eq!(refused["ok"], json!(false), "{text}");
            assert_eq!(refused["reason"], json!("not-an-address"));
            let expected = translate("page.spend.refusal.not_an_address", lang).replace("{found}", "claude.ai/x");
            assert_eq!(refused["hint"], json!(expected), "{text}: the hint speaks the project language");
        }
        assert!(!machine.dir.as_ref().unwrap().exists(), "a refusal writes nothing");

        let root = project(dir.path(), "ok", "pt-BR");
        let dir_of = machine.dir.clone().unwrap();
        let record = |url: &str| spend_at(&SpendOpts { url: Some(url.into()), ..opts(&root) }, &machine);
        assert_eq!(record("https://claude.ai/code/artifact/a")["ok"], json!(true));
        store::update(&dir_of, |ledger| {
            ledger.prepared_through = Some("2026-10-01".into());
            ledger.copied_through = Some("2026-09-30".into());
        })
        .unwrap();
        assert_eq!(record("https://claude.ai/code/artifact/a")["ok"], json!(true));
        assert_eq!(store::load(&dir_of).unwrap().copied_through.as_deref(), Some("2026-09-30"), "the same address keeps the mark");
        assert_eq!(record("https://claude.ai/code/artifact/b")["ok"], json!(true));
        let ledger = store::load(&dir_of).unwrap();
        assert_eq!(ledger.copied_through, None, "a new page has an empty database");
        assert_eq!(ledger.prepared_through.as_deref(), Some("2026-10-01"), "what was prepared stays for the copy that follows");
        assert_eq!(ledger.url.as_deref(), Some("https://claude.ai/code/artifact/b"));
    }

    /// Todas as escritas da cópia preparada, de todos os lotes.
    fn writes_of(answer: &Value) -> Vec<Value> {
        answer["copy"]["spend"]["writes"].as_array().unwrap().iter().flat_map(|b| b.as_array().unwrap().clone()).collect()
    }

    /// A cópia só se grava como feita depois de preparada; depois do lote, o
    /// último dia dele vira o último dia copiado, cada documento dele sobe
    /// uma versão, e as linhas fechadas não vão de novo.
    #[test]
    fn a_copy_is_recorded_as_done_only_after_it_was_prepared() {
        let dir = tempfile::tempdir().unwrap();
        let machine = machine(dir.path(), "2026-10-02");
        let root = project(dir.path(), "loja", "pt-BR");
        let copied = SpendOpts { copied: true, ..opts(&root) };
        let refused = spend_at(&copied, &machine);
        assert_eq!(refused["reason"], json!("nothing-prepared"), "{refused}");
        assert!(refused["hint"].as_str().unwrap().contains("mustard-rt run spend"));

        conversation(dir.path(), &root, &["2026-09-30", "2026-10-01"]);
        let answer = spend_at(&opts(&root), &machine);
        assert_eq!(answer["ok"], json!(true), "{answer}");
        assert_eq!(answer["counted"]["rows"], json!(2));
        let dir_of = machine.dir.clone().unwrap();
        let ledger = store::load(&dir_of).unwrap();
        assert_eq!((ledger.counted_through.as_deref(), ledger.prepared_through.as_deref()), (Some("2026-10-01"), Some("2026-10-01")));
        assert_eq!(ledger.uncopied().len(), 2, "the lines are counted and not yet copied");
        assert!(ledger.versions.is_empty(), "the database has nothing yet");

        assert_eq!(spend_at(&copied, &machine)["ok"], json!(true));
        let ledger = store::load(&dir_of).unwrap();
        assert_eq!(ledger.copied_through.as_deref(), Some("2026-10-01"));
        assert!(ledger.uncopied().is_empty(), "nothing left to copy today");
        let names: Vec<&str> = ledger.versions.keys().map(String::as_str).collect();
        assert_eq!(names, ["days/2026-09-30-loja", "days/2026-10-01-loja", "summary/current"]);
        assert!(ledger.versions.values().all(|version| *version == 1), "{:?}", ledger.versions);
        assert_eq!(spend_at(&copied, &machine)["reason"], json!("nothing-prepared"), "a copy is recorded once");
    }

    /// Hoje é contado de novo a cada pedido e vai à cópia como linha parcial,
    /// mas nunca ao arquivo dos dias fechados: o arquivo para em ontem. Depois
    /// de uma cópia gravada, a linha de hoje e o resumo voltam com a versão
    /// que o banco tem, e as linhas fechadas já copiadas não voltam.
    #[test]
    fn today_is_recounted_on_each_run_and_never_reaches_the_closed_file() {
        let dir = tempfile::tempdir().unwrap();
        let machine = machine(dir.path(), "2026-10-02");
        let root = project(dir.path(), "loja", "pt-BR");
        conversation(dir.path(), &root, &["2026-10-01", "2026-10-02"]);
        let dir_of = machine.dir.clone().unwrap();

        let first = spend_at(&opts(&root), &machine);
        assert_eq!(first["ok"], json!(true), "{first}");
        let writes = writes_of(&first);
        let docs: Vec<&str> = writes.iter().map(|w| w["doc_id"].as_str().unwrap()).collect();
        assert_eq!(docs, ["2026-10-01-loja", "2026-10-02-loja", "current"], "the closed day, the open day and the summary");
        let body = |write: &Value| -> Value { serde_json::from_str(&fs::read_to_string(write["file_path"].as_str().unwrap()).unwrap()).unwrap() };
        assert_eq!((body(&writes[0])["partial"].clone(), body(&writes[1])["partial"].clone()), (json!(false), json!(true)));
        let ledger = store::load(&dir_of).unwrap();
        assert_eq!(ledger.counted_through.as_deref(), Some("2026-10-01"));
        assert!(ledger.rows.iter().all(|row| row.day.as_str() <= "2026-10-01"), "today is not in the closed file: {:?}", ledger.rows);
        assert!(writes.iter().all(|w| w.get("if_version").is_none()), "the first copy goes to an empty database");

        assert_eq!(spend_at(&SpendOpts { copied: true, ..opts(&root) }, &machine)["ok"], json!(true));
        conversation(dir.path(), &root, &["2026-10-01", "2026-10-02", "2026-10-02"]);
        let second = spend_at(&opts(&root), &machine);
        let again = writes_of(&second);
        let docs: Vec<&str> = again.iter().map(|w| w["doc_id"].as_str().unwrap()).collect();
        assert_eq!(docs, ["2026-10-02-loja", "current"], "the closed line was copied already; today and the summary are sent again: {second}");
        assert_eq!(body(&again[0])["tokens"], json!(20), "today is counted again from the conversations");
        assert_eq!(again[0]["if_version"], json!(1), "the partial line is replaced with the version the database has");
        assert_eq!(again[1]["if_version"], json!(1));
        let ledger = store::load(&dir_of).unwrap();
        assert!(ledger.rows.iter().all(|row| row.day.as_str() <= "2026-10-01"), "still not in the closed file");
    }

    /// A publicação nova leva todos os dias fechados mais hoje, sem versão em
    /// nenhuma escrita, porque o banco da página nova está vazio; o endereço
    /// novo zera as versões, e a cópia que vem depois as começa de novo.
    #[test]
    fn republishing_sends_every_day_to_an_empty_database() {
        let dir = tempfile::tempdir().unwrap();
        let machine = machine(dir.path(), "2026-10-02");
        let root = project(dir.path(), "loja", "pt-BR");
        conversation(dir.path(), &root, &["2026-09-29", "2026-09-30", "2026-10-01", "2026-10-02"]);
        let dir_of = machine.dir.clone().unwrap();
        let record = |url: &str| spend_at(&SpendOpts { url: Some(url.into()), ..opts(&root) }, &machine);
        let copied = SpendOpts { copied: true, ..opts(&root) };

        assert_eq!(spend_at(&opts(&root), &machine)["ok"], json!(true));
        assert_eq!(record("https://claude.ai/code/artifact/a")["ok"], json!(true));
        assert_eq!(spend_at(&copied, &machine)["ok"], json!(true));
        let quiet = spend_at(&opts(&root), &machine);
        assert_eq!(writes_of(&quiet).len(), 2, "with nothing new only today and the summary go: {quiet}");
        assert_eq!(spend_at(&copied, &machine)["ok"], json!(true));
        assert!(store::load(&dir_of).unwrap().versions.values().all(|version| *version >= 1));

        let all = spend_at(&SpendOpts { republish: true, ..opts(&root) }, &machine);
        assert_eq!(all["ok"], json!(true), "{all}");
        let writes = writes_of(&all);
        let docs: Vec<&str> = writes.iter().map(|w| w["doc_id"].as_str().unwrap()).collect();
        assert_eq!(
            docs,
            ["2026-09-29-loja", "2026-09-30-loja", "2026-10-01-loja", "2026-10-02-loja", "current"],
            "every closed day, today and the summary"
        );
        assert!(writes.iter().all(|w| w.get("if_version").is_none()), "a new page has an empty database: {all}");
        assert!(all["order"][0].as_str().unwrap().contains("page.html"), "the first step publishes the page again: {all}");

        assert_eq!(record("https://claude.ai/code/artifact/b")["ok"], json!(true));
        assert!(store::load(&dir_of).unwrap().versions.is_empty(), "the new address starts the versions over");
        assert_eq!(spend_at(&copied, &machine)["ok"], json!(true));
        let ledger = store::load(&dir_of).unwrap();
        assert_eq!(ledger.copied_through.as_deref(), Some("2026-10-01"));
        assert!(ledger.versions.values().all(|version| *version == 1), "{:?}", ledger.versions);
    }

    /// A resposta traz o resumo da máquina: hoje até agora, marcado como
    /// parcial, e ontem.
    #[test]
    fn the_copy_carries_the_machine_summary_with_today_so_far() {
        let dir = tempfile::tempdir().unwrap();
        let machine = machine(dir.path(), "2026-10-02");
        let root = project(dir.path(), "loja", "en-US");
        conversation(dir.path(), &root, &["2026-10-01", "2026-10-02"]);
        let answer = spend_at(&opts(&root), &machine);
        let writes = writes_of(&answer);
        let summary = writes.iter().find(|w| w["collection"] == json!("summary")).unwrap();
        let body: Value = serde_json::from_str(&fs::read_to_string(summary["file_path"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(body["today"]["day"], json!("2026-10-02"));
        assert_eq!(body["today"]["tokens"], json!(10));
        assert_eq!(body["yesterday"]["day"], json!("2026-10-01"));
        assert_eq!(body["yesterday"]["tokens"], json!(10));
    }

    /// Sem a pasta pessoal para guardar o gasto, o comando recusa nos dois
    /// idiomas e não escreve nada.
    #[test]
    fn without_a_machine_folder_the_command_refuses_in_both_languages() {
        let dir = tempfile::tempdir().unwrap();
        for (text, lang) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
            let root = project(dir.path(), text, text);
            let machine = Machine { dir: None, config: None, today: "2026-10-02".into() };
            let refused = spend_at(&opts(&root), &machine);
            assert_eq!(refused["reason"], json!("no-machine-folder"));
            assert_eq!(refused["hint"], json!(translate("page.spend.refusal.no_machine_folder", lang)), "{text}");
        }
    }

    /// Um arquivo do gasto que não se lê é recusado sem ser tocado, e o
    /// conserto está na mensagem: apagar o arquivo.
    #[test]
    fn an_unreadable_ledger_is_refused_untouched_with_the_way_out() {
        let dir = tempfile::tempdir().unwrap();
        let machine = machine(dir.path(), "2026-10-02");
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
