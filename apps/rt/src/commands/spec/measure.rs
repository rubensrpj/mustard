//! `mustard-rt run measure` — o gasto do Claude no projeto antes e depois da
//! marca de uma versão do Mustard.
//!
//! Sem `--since`, a marca é a última que o início de sessão gravou no
//! projeto; com ele, o instante dado, em RFC 3339 ou `AAAA-MM-DD` (a
//! meia-noite do dia no fuso do gasto). A conta mora em
//! `mustard_core::domain::measure`, sobre as linhas de dia que
//! `mustard_core::io::spend` dá ao projeto, pela mesma soma da página do
//! gasto. O comando só lê: não grava nada e não chama o Jev. O veredito sai
//! numa frase, no idioma do projeto, em `text`.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use mustard_core::domain::measure::{measure, Mark, Measurement, Verdict, MIN_DAYS};
use mustard_core::domain::spend::{day_start, Range};
use mustard_core::io::{measure as marks, spend as store};
use mustard_core::platform::harness::claude_config_dir;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

use crate::commands::spec_events::project;

/// Options for `mustard-rt run measure`.
pub struct MeasureOpts {
    /// Qualquer pasta dentro do projeto medido.
    pub root: PathBuf,
    /// O instante da marca, no lugar da última marca do projeto, como veio
    /// na linha de comando.
    pub since: Option<String>,
}

/// O instante de `--since`: um carimbo RFC 3339, ou um dia `AAAA-MM-DD`, que
/// começa à meia-noite no fuso do gasto; o texto que não é nenhum dos dois dá
/// a mensagem da recusa.
fn since(text: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(text)
        .map(|at| at.with_timezone(&Utc))
        .ok()
        .or_else(|| day_start(text))
        .ok_or_else(|| format!("`{text}` is neither an RFC 3339 instant nor an AAAA-MM-DD day"))
}

/// O núcleo testável de [`run`]: a resposta do comando, com as conversas do
/// Claude Code em `config` e hoje em `today`. Sem marca e sem `--since`, só a
/// frase de que a medição começa na próxima sessão. O `--since` que não é
/// instante nem dia é recusado, sem ler nada.
pub(crate) fn measure_at(opts: &MeasureOpts, config: Option<&Path>, today: &str) -> Value {
    let lang = project(&opts.root).lang;
    let name = store::project_name(&opts.root);
    let mark = match opts.since.as_deref().map(since) {
        Some(Err(hint)) => return json!({ "ok": false, "reason": "not-an-instant", "hint": hint }),
        Some(Ok(at)) => Some((json!({ "version": null, "at": at.to_rfc3339() }), at)),
        None => marks::marks(&opts.root).pop().and_then(|mark: Mark| {
            let at = DateTime::parse_from_rfc3339(&mark.at).ok()?.with_timezone(&Utc);
            Some((json!(mark), at))
        }),
    };
    let Some((shown, at)) = mark else {
        return json!({ "ok": true, "project": name, "mark": null, "text": translate("page.measure.no_mark", lang) });
    };
    let range = Range { first: None, last: today.to_string() };
    let rows = config.map(|config| store::project_days(config, &opts.root, &range)).unwrap_or_default();
    let measured = measure(&rows, at, today);
    json!({
        "ok": true,
        "project": name,
        "mark": shown,
        "before": measured.before,
        "after": measured.after,
        "verdict": measured.verdict,
        "text": sentence(&measured, lang),
    })
}

/// O veredito de `measured` numa frase, em `lang`.
fn sentence(measured: &Measurement, lang: Locale) -> String {
    let fill = |key: &str, slots: &[(&str, String)]| {
        slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
    };
    match measured.verdict {
        Verdict::NotUsedYet => fill("page.measure.not_used_yet", &[]),
        Verdict::TooEarly { missing_before, missing_after } => fill(
            "page.measure.too_early",
            &[
                ("{missing_before}", missing_before.to_string()),
                ("{missing_after}", missing_after.to_string()),
                ("{min}", MIN_DAYS.to_string()),
            ],
        ),
        Verdict::Ready => fill(
            "page.measure.ready",
            &[
                ("{before}", measured.before.tokens_per_action.to_string()),
                ("{before_days}", measured.before.days.len().to_string()),
                ("{after}", measured.after.tokens_per_action.to_string()),
                ("{after_days}", measured.after.days.len().to_string()),
            ],
        ),
    }
}

/// CLI entry — `mustard-rt run measure`.
pub fn run(opts: &MeasureOpts) {
    let report = measure_at(opts, claude_config_dir().as_deref(), &store::today());
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

    /// Sem marca, a resposta diz, no idioma do projeto, que a medição começa
    /// na próxima sessão; com a marca da sessão, a conta parte dela, e a
    /// versão ainda sem dia depois dela não foi usada; o `--since` que não é
    /// instante nem dia é recusado; com um dia, ele fica fora dos dois lados,
    /// o antes tem o tamanho do depois e a frase diz quantos dias faltam a
    /// cada lado.
    #[test]
    fn the_command_measures_from_the_last_mark_or_the_given_instant_in_the_project_language() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        // Um projeto com `mustard.json` na pasta `name`, no idioma `text`.
        let project = |name: &str, text: &str| {
            let root = dir.path().join(name);
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{text}"}}}}"#)).unwrap();
            root
        };
        for (text, lang) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
            let root = project(text, text);
            let answer = measure_at(&MeasureOpts { root, since: None }, Some(&config), "2026-10-04");
            assert_eq!(answer["mark"], Value::Null, "{text}: {answer}");
            assert_eq!(answer["text"], json!(translate("page.measure.no_mark", lang)), "{text}");
        }

        let root = project("loja", "pt-BR");
        // Uma conversa do projeto com uma resposta de 100 ações e mil tokens
        // em cada dia.
        let days = ["2026-09-28", "2026-09-29", "2026-09-30", "2026-10-01", "2026-10-02", "2026-10-03", "2026-10-04"];
        let lines: Vec<String> = days
            .iter()
            .map(|day| {
                let tools: Vec<Value> =
                    (0..100).map(|n| json!({"type": "tool_use", "id": format!("{day}-{n}"), "name": "Read", "input": {}})).collect();
                json!({"timestamp": format!("{day}T15:00:00Z"), "cwd": root.to_string_lossy(), "message": {
                    "id": day, "model": "m", "usage": {"input_tokens": 1000, "output_tokens": 0}, "content": tools}})
                .to_string()
            })
            .collect();
        fs::create_dir_all(config.join("projects/p")).unwrap();
        fs::write(config.join("projects/p/s1.jsonl"), lines.join("\n")).unwrap();
        marks::record(&root, "1.0 (abc)").unwrap();
        let marked = measure_at(&MeasureOpts { root: root.clone(), since: None }, Some(&config), "2026-10-04");
        assert_eq!((marked["project"].clone(), marked["mark"]["version"].clone()), (json!("loja"), json!("1.0 (abc)")));
        assert_eq!(marked["verdict"], json!({"kind": "not-used-yet"}), "{marked}");
        assert_eq!(marked["text"], json!(translate("page.measure.not_used_yet", Locale::PtBr)));

        let refused = measure_at(&MeasureOpts { root: root.clone(), since: Some("01/10".into()) }, Some(&config), "2026-10-04");
        assert_eq!((refused["ok"].clone(), refused["reason"].clone()), (json!(false), json!("not-an-instant")), "{refused}");
        let given = measure_at(&MeasureOpts { root, since: Some("2026-10-01".into()) }, Some(&config), "2026-10-04");
        assert_eq!(given["before"]["days"], json!(["2026-09-29", "2026-09-30"]), "{given}");
        assert_eq!(given["after"]["days"], json!(["2026-10-02", "2026-10-03"]), "{given}");
        assert_eq!((given["after"]["actions"].clone(), given["after"]["tokens_per_action"].clone()), (json!(200), json!(10)));
        let expected = "Ainda não dá para dizer: faltam 3 dias contados antes e 3 depois para o mínimo de 5 de cada lado.";
        assert_eq!(given["text"], json!(expected));
    }
}
