//! `mustard-rt run measure` — o gasto do Claude no projeto antes e depois da
//! marca de uma versão do Mustard.
//!
//! Sem `--since`, a marca é a última que o início de sessão gravou no
//! projeto; com ele, o instante dado, em RFC 3339 ou `AAAA-MM-DD` (a
//! meia-noite do dia no fuso do gasto). A conta mora em
//! `mustard_core::domain::measure`, sobre as linhas de dia que
//! `mustard_core::io::spend` dá ao projeto, pela mesma soma da página do
//! gasto, só das conversas abertas na pasta do projeto e nas cópias de onda
//! dele. O comando só lê: não grava nada e não chama o Jev. Em `text`, no
//! idioma do projeto, saem a tabela do gasto e a frase do veredito.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use mustard_core::domain::measure::{measure, Mark, Measurement, Side, Verdict, MIN_DAYS};
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

/// A resposta do comando, com as conversas do Claude Code em `config` e hoje
/// em `today`. Sem marca e sem `--since`, só a frase de que a medição começa
/// na próxima sessão. O `--since` que não é instante nem dia é recusado, sem
/// ler nada.
fn measure_at(opts: &MeasureOpts, config: Option<&Path>, today: &str) -> Value {
    let lang = project(&opts.root).lang;
    let place = store::project_place(&opts.root);
    let name = place.as_deref().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned());
    let mark = match opts.since.as_deref().map(since) {
        Some(Err(hint)) => return json!({ "ok": false, "reason": "not-an-instant", "hint": hint }),
        Some(Ok(at)) => Some((json!({ "version": null, "at": at.to_rfc3339() }), at)),
        None => marks::marks(&opts.root).pop().and_then(|mark: Mark| {
            let at = DateTime::parse_from_rfc3339(&mark.at).ok()?.with_timezone(&Utc);
            Some((json!(mark), at))
        }),
    };
    let Some((shown, at)) = mark else {
        return json!({ "ok": true, "project": name, "mark": null, "text": translate("measure.no_mark", lang) });
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
        "text": format!("{}\n\n{}", table(&measured, lang), sentence(&measured, lang)),
    })
}

/// O texto de `key` em `lang`, com cada lacuna de `slots` trocada.
fn fill(key: &str, lang: Locale, slots: &[(&str, String)]) -> String {
    slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
}

/// `number` com os milhares separados no costume de `lang`.
fn grouped(number: u64, lang: Locale) -> String {
    let mark = match lang {
        Locale::PtBr => '.',
        Locale::EnUs => ',',
    };
    let digits = number.to_string();
    let mut shown = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            shown.push(mark);
        }
        shown.push(digit);
    }
    shown
}

/// `number` em mil (`key` de mil) ou em milhões, arredondado e com os
/// milhares separados.
fn scaled(number: u64, unit: u64, key: &str, lang: Locale) -> String {
    fill(key, lang, &[("{n}", grouped(number.saturating_add(unit / 2) / unit, lang))])
}

/// Os dias `days` (`AAAA-MM-DD`) numa lista curta, com o mês só no último
/// dia de cada mês: `23, 24 e 30/09`.
fn day_list(days: &[String], lang: Locale) -> String {
    let month = |day: &str| day.get(5..7).unwrap_or_default().to_string();
    let shown: Vec<String> = days
        .iter()
        .enumerate()
        .map(|(at, day)| {
            let date = day.get(8..10).unwrap_or_default();
            let closes = days.get(at + 1).is_none_or(|next| month(next) != month(day));
            if closes { format!("{date}/{}", month(day)) } else { date.to_string() }
        })
        .collect();
    match shown.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} {} {last}", rest.join(", "), translate("measure.and", lang)),
        None => String::new(),
    }
}

/// A tabela do gasto de `measured`, em `lang`: dias contados, ações, tokens
/// em milhões e tokens por ação em mil, antes e depois. O lado sem dia
/// contado fica sem uso.
fn table(measured: &Measurement, lang: Locale) -> String {
    let head = |key: &str, side: &Side| match side.days.as_slice() {
        [] => translate(key, lang).to_string(),
        days => format!("{} ({})", translate(key, lang), day_list(days, lang)),
    };
    let cells = |side: &Side| -> [String; 4] {
        if side.days.is_empty() {
            let unused = translate("measure.unused", lang).to_string();
            return ["0".to_string(), unused.clone(), unused.clone(), unused];
        }
        [
            side.days.len().to_string(),
            grouped(side.actions, lang),
            scaled(side.tokens, 1_000_000, "measure.millions", lang),
            format!("**{}**", scaled(side.tokens_per_action, 1_000, "measure.thousands", lang)),
        ]
    };
    let (before, after) = (cells(&measured.before), cells(&measured.after));
    let rows = ["measure.days", "measure.actions", "measure.tokens", "measure.per_action"];
    let (left, right) = (head("measure.before", &measured.before), head("measure.after", &measured.after));
    let lines = rows.iter().enumerate().map(|(at, key)| {
        let label = if at == 3 { format!("**{}**", translate(key, lang)) } else { translate(key, lang).to_string() };
        format!("\n| {label} | {} | {} |", before[at], after[at])
    });
    format!("| | {left} | {right} |\n|---|---:|---:|{}", lines.collect::<String>())
}

/// O veredito de `measured` numa frase, em `lang`, com quanto cada ação
/// custou a mais ou a menos depois da marca quando os dois lados têm dia.
fn sentence(measured: &Measurement, lang: Locale) -> String {
    let (before, after) = (&measured.before, &measured.after);
    let change = || {
        let (was, is) = (before.tokens_per_action, after.tokens_per_action);
        let percent = (was.abs_diff(is).saturating_mul(100).saturating_add(was / 2)).checked_div(was).unwrap_or(0);
        let key = match is.cmp(&was) {
            _ if percent == 0 => "measure.same",
            std::cmp::Ordering::Greater => "measure.more",
            _ => "measure.less",
        };
        fill(key, lang, &[("{percent}", percent.to_string())])
    };
    let days = [("{before_days}", before.days.len().to_string()), ("{after_days}", after.days.len().to_string())];
    match measured.verdict {
        Verdict::NotUsedYet => fill("measure.not_used_yet", lang, &[]),
        Verdict::TooEarly { missing_before, missing_after } => {
            let missing = [
                ("{missing_before}", missing_before.to_string()),
                ("{missing_after}", missing_after.to_string()),
                ("{min}", MIN_DAYS.to_string()),
            ];
            let said = fill("measure.too_early", lang, &[days.as_slice(), &missing].concat());
            if before.days.is_empty() {
                return said;
            }
            format!("{said} {}", fill("measure.so_far", lang, &[("{change}", change())]))
        }
        Verdict::Ready => fill("measure.ready", lang, &[days.as_slice(), &[("{change}", change())]].concat()),
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
