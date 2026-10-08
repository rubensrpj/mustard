//! `mustard-rt run spend` — o gasto de cada dia, contado pelas conversas e
//! mostrado numa página.
//!
//! Sem argumento, conta e guarda dias fechados e mostra o resumo local de hoje.
//! Só `--publish` ou `--republish` prepara e publica um snapshot estático. `--url`
//! registra uma URL confirmada. Nenhum hook inicia essa operação.
//!
//! A regra mora em `mustard_core::domain::spend` e `mustard_core::io::spend`.
//! Recusa sai com exit 1, `ok: false`, a razão em `reason` e a mensagem no
//! idioma do projeto em `hint`.

use std::path::{Path, PathBuf};

use mustard_core::domain::spend::{Refusal, summarize};
use mustard_core::io::spend as store;
use mustard_core::platform::harness::claude_config_dir;
use mustard_core::platform::i18n::Locale;
use serde_json::{Value, json};

use crate::commands::spec_events::project;

/// Options for `mustard-rt run spend`.
pub struct SpendOpts {
    /// Qualquer pasta dentro do repositório: só dá o idioma das mensagens.
    pub root: PathBuf,
    /// Prepara a página externa somente a pedido explícito.
    pub publish: bool,
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
        Self {
            dir: store::machine_dir(),
            config: claude_config_dir(),
            today: store::today(),
        }
    }
}

/// O núcleo testável de [`run`]: a resposta do comando ou a recusa.
pub(crate) fn spend_at(opts: &SpendOpts, machine: &Machine) -> Value {
    let lang = project(&opts.root).lang;
    match answer(opts, machine, lang) {
        Ok(report) => report,
        Err(refusal) => {
            json!({ "ok": false, "reason": refusal.reason(), "hint": refusal.message(lang) })
        }
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
    let open = store::count_open(config, Some(dir), &machine.today);
    store::update(dir, |ledger| {
        ledger.open_rows.clone_from(&open);
        ledger.open_day=Some(machine.today.clone());
        ledger.measured_at=Some(chrono::Utc::now().to_rfc3339());
    })?;
    if !opts.publish && !opts.republish {
        let ledger = store::load(dir)?;
        return Ok(
            json!({"ok":true,"counted":counted,"summary":summarize(&ledger.rows,&open,&machine.today),"published":false}),
        );
    }
    let ledger = store::update(dir, |ledger| ledger.clone())?;
    let rows=ledger.rows.iter().chain(&open).map(|row| {
        let name=row.project.rsplit(['/', '\\']).next().unwrap_or("Projeto");
        let project=crate::shared::secret::without_secrets(&name.chars().filter(|c|!c.is_control()).collect::<String>());
        json!({"day":row.day,"project":project,"tokens":row.tokens,"actions":row.actions,"code_searches":row.code_searches,
            "jev_tokens":row.jev_tokens,"jev_cost_micro_usd":row.jev_cost_micro_usd,"partial":row.partial})
    }).collect::<Vec<_>>();
    let public = json!({"schema_version":1,"kind":"spend","language":lang.to_string(),"at":chrono::Utc::now().to_rfc3339(),
        "rows":rows,"summary":summarize(&ledger.rows,&open,&machine.today)});
    let prepared = crate::shared::publication::prepare_snapshot(
        dir,
        &dir.join("publications"),
        &public,
        crate::report::public_spend_snapshot,
    );
    let mut answer = crate::shared::publication::upload_prepared(
        &opts.root,
        dir,
        prepared,
        "spend",
        crate::report::public_spend_snapshot,
    );
    answer["counted"] = counted;
    if answer["published"] == true {
        // Static snapshots have no remote document versions/copy cursor.
        let url = answer["remote_url"].as_str().ok_or_else(|| Refusal::Io {
            detail: "publication-no-confirmed-url".into(),
        })?;
        store::update(dir, |ledger| ledger.url = Some(url.to_string()))?;
    }
    Ok(answer)
}

/// Conta os dias fechados que faltam e guarda as linhas no arquivo da
/// máquina; devolve a faixa contada e quantas linhas ela deu, ou `null` quando
/// não faltava dia nenhum. Um dia já contado não se abre de novo.
fn count_missing(dir: &Path, config: &Path, today: &str) -> Result<Value, Refusal> {
    let Some(range) = store::load(dir)?.to_count(today) else {
        return Ok(Value::Null);
    };
    let rows = store::count(config, Some(dir), &range);
    let counted = rows.len();
    store::update(dir, |ledger| ledger.record_counted(&range, rows))?;
    Ok(json!({ "first": range.first, "last": range.last, "rows": counted }))
}

/// Grava o endereço da página.
fn record_url(dir: &Path, url: &str) -> Result<Value, Refusal> {
    let url = url.trim();
    if !url.starts_with("https://") || url.len() <= "https://".len() {
        return Err(Refusal::NotAnAddress {
            found: url.to_string(),
        });
    }
    store::update(dir, |ledger| ledger.url = Some(url.to_string()))?;
    Ok(json!({ "ok": true, "recorded": "url" }))
}

/// CLI entry — `mustard-rt run spend`.
pub fn run(opts: &SpendOpts) {
    let report = spend_at(opts, &Machine::here());
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
    );
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
        Machine {
            dir: Some(base.join("spend")),
            config: Some(base.join("config")),
            today: today.to_string(),
        }
    }

    fn opts(root: &Path) -> SpendOpts {
        SpendOpts {
            root: root.to_path_buf(),
            publish: true,
            republish: false,
            url: None,
        }
    }

    /// Um projeto com `mustard.json` na pasta `name` de `base`, no idioma
    /// `text`.
    fn project(base: &Path, name: &str, text: &str) -> PathBuf {
        let root = base.join(name);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("mustard.json"),
            format!(r#"{{"language":{{"text":"{text}"}}}}"#),
        )
        .unwrap();
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

    fn database(answer: &Value) -> Value {
        serde_json::from_slice(&fs::read(answer["database"].as_str().unwrap()).unwrap()).unwrap()
    }

    #[test]
    fn an_explicit_static_snapshot_is_complete_retryable_and_does_not_confirm_an_upload() {
        let dir = tempfile::tempdir().unwrap();
        let machine = machine(dir.path(), "2026-10-02");
        let root = project(dir.path(), "loja", "pt-BR");
        conversation(
            dir.path(),
            &root,
            &["2026-09-30", "2026-10-01", "2026-10-02"],
        );
        let first = spend_at(&opts(&root), &machine);
        assert_eq!(first["ok"], true, "{first}");
        assert_eq!(first["counted"]["rows"], 2);
        assert_eq!(first["published"], false);
        assert_eq!(first["reason"], "publication-not-configured");
        let data = database(&first);
        assert_eq!(data["rows"].as_array().unwrap().len(), 3);
        assert_eq!(data["rows"][2]["partial"], true);
        assert!(!first.to_string().contains("ArtifactData") && first.get("order").is_none());
        let again = spend_at(&opts(&root), &machine);
        assert_eq!(first["snapshot_id"], again["snapshot_id"]);
        assert_eq!(
            database(&again),
            data,
            "unchanged export retains its timestamp"
        );
        assert!(again["counted"].is_null());
        spend_at(
            &SpendOpts {
                url: Some("https://example.com/historical".into()),
                ..opts(&root)
            },
            &machine,
        );
        let all = spend_at(
            &SpendOpts {
                republish: true,
                ..opts(&root)
            },
            &machine,
        );
        assert_eq!(database(&all)["rows"], data["rows"]);
        assert_eq!(
            all["published"], false,
            "a historical URL is no upload confirmation"
        );
        let ledger = store::load(machine.dir.as_ref().unwrap()).unwrap();
        assert!(ledger.copied_through.is_none() && ledger.versions.is_empty());
        let html = fs::read_to_string(all["page"].as_str().unwrap()).unwrap();
        assert!(html.contains("status-grid") && !html.contains("@claude/artifact-runtime"));
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
            let refused = spend_at(
                &SpendOpts {
                    url: Some("claude.ai/x".into()),
                    ..opts(&root)
                },
                &machine,
            );
            assert_eq!(
                (refused["ok"].clone(), refused["reason"].clone()),
                (json!(false), json!("not-an-address")),
                "{text}"
            );
            let expected = translate("page.spend.refusal.not_an_address", lang)
                .replace("{found}", "claude.ai/x");
            assert_eq!(
                refused["hint"],
                json!(expected),
                "{text}: the hint speaks the project language"
            );
            let no_folder = spend_at(
                &opts(&root),
                &Machine {
                    dir: None,
                    config: None,
                    today: "2026-10-02".into(),
                },
            );
            assert_eq!(no_folder["reason"], json!("no-machine-folder"));
            assert_eq!(
                no_folder["hint"],
                json!(translate("page.spend.refusal.no_machine_folder", lang)),
                "{text}"
            );
        }
        assert!(
            !machine.dir.as_ref().unwrap().exists(),
            "a refusal writes nothing"
        );

        let root = project(dir.path(), "loja", "en-US");
        let folder = machine.dir.clone().unwrap();
        fs::create_dir_all(&folder).unwrap();
        fs::write(store::ledger_path(&folder), "{ not json").unwrap();
        let refused = spend_at(&opts(&root), &machine);
        assert_eq!(refused["reason"], json!("unreadable-ledger"), "{refused}");
        assert!(
            refused["hint"]
                .as_str()
                .unwrap()
                .contains("Delete the file"),
            "{refused}"
        );
        assert_eq!(
            fs::read_to_string(store::ledger_path(&folder)).unwrap(),
            "{ not json"
        );
    }
}
