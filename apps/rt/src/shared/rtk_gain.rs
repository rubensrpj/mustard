//! `rtk gain` normalisation — a port of `scripts/_rtk-gain.js`.
//!
//! The JS `_rtk-gain.js` was a shared helper, not a standalone script: it
//! shells `rtk gain --all --format json` and normalises the result across rtk
//! versions. This module keeps it as a helper consumed by `run statusline`.
//!
//! [`project_days`] é a outra leitura: a economia do rtk neste projeto, dia a
//! dia (`rtk gain -p -d -f json`, rodado na pasta do projeto), que o painel da
//! página da spec soma nos dias da spec. Só entram os dias já fechados: o de
//! hoje muda a cada comando, e duas gerações da mesma página têm de sair
//! iguais. O Mustard não conta nada por conta própria: os números são os do
//! rtk.
//!
//! Fail-open: `rtk` missing, a timeout, or unparseable JSON yields `None`,
//! exactly like the JS helper returning `null`.

use mustard_core::view::document::RtkDay;
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

/// Normalised `rtk gain` summary — the fields the statusline segment consumes.
#[derive(Debug, Clone)]
pub struct RtkGain {
    /// Total tokens saved by RTK rewrites.
    pub saved: i64,
    /// Average savings percentage.
    pub pct: f64,
}

/// Read a numeric field from a `serde_json` object, tolerating string numbers
/// and the alternate key spellings `_rtk-gain.js` accepted.
fn num(obj: &Value, keys: &[&str]) -> f64 {
    for key in keys {
        if let Some(v) = obj.get(*key) {
            if let Some(n) = v.as_f64() {
                return n;
            }
            if let Some(s) = v.as_str()
                && let Ok(n) = s.parse::<f64>() {
                    return n;
                }
        }
    }
    0.0
}

/// Shell `rtk gain --all --format json` and normalise the result.
///
/// Returns `None` on any failure (rtk absent, non-zero exit, bad JSON), or
/// when both `saved` and `commands` are non-positive — the JS guard
/// `if (saved <= 0 && commands <= 0) return null`.
#[must_use]
pub fn get_rtk_gain() -> Option<RtkGain> {
    let output = Command::new("rtk")
        .args(["gain", "--all", "--format", "json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?;
    let data: Value = serde_json::from_str(&raw).ok()?;
    // The JS reads `data.summary` when present, else `data` itself.
    let summary = data.get("summary").unwrap_or(&data);

    let saved = num(summary, &["total_saved", "saved_tokens", "savedTokens"]) as i64;
    let pct = num(summary, &["avg_savings_pct", "savings_pct", "savingsPct"]);
    let commands = num(summary, &["total_commands", "commands"]) as i64;

    if saved <= 0 && commands <= 0 {
        return None;
    }
    Some(RtkGain { saved, pct })
}

/// A economia do rtk no projeto `root`, um dia por linha, só dos dias antes
/// de `before` (`2026-09-17`). Sem o rtk, com uma saída que não se entende ou
/// sem comando nenhum no projeto, nenhum dia.
#[must_use]
pub fn project_days(root: &Path, before: &str) -> Vec<RtkDay> {
    let Ok(output) = Command::new("rtk")
        .args(["gain", "--project", "--daily", "--format", "json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let days = String::from_utf8(output.stdout).map(|raw| parse_days(&raw)).unwrap_or_default();
    closed(days, before)
}

/// Só os dias antes de `before`.
fn closed(days: Vec<RtkDay>, before: &str) -> Vec<RtkDay> {
    days.into_iter().filter(|day| day.date.as_str() < before).collect()
}

/// Os dias da saída JSON do `rtk gain --daily`, em ordem de data. O dia sem
/// data fica de fora.
fn parse_days(raw: &str) -> Vec<RtkDay> {
    let Ok(data) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let whole = |v: &Value, keys: &[&str]| num(v, keys).max(0.0) as u64;
    let mut days: Vec<RtkDay> = data
        .get("daily")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|day| {
            let date = day.get("date").and_then(Value::as_str)?.trim();
            (date.len() == 10).then(|| RtkDay {
                date: date.to_string(),
                commands: whole(day, &["commands", "total_commands"]),
                input: whole(day, &["input_tokens", "total_input"]),
                saved: whole(day, &["saved_tokens", "total_saved"]),
            })
        })
        .collect();
    days.sort_by(|a, b| a.date.cmp(&b.date));
    days
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Cada dia da saída do rtk vira um dia com os números dele, em ordem de
    /// data; uma saída sem dias, ou que não se entende, não dá dia nenhum.
    #[test]
    fn the_daily_savings_come_from_the_rtk_numbers() {
        let raw = json!({
            "summary": {"total_commands": 3},
            "daily": [
                {"date": "2026-09-12", "commands": 2, "input_tokens": 1000, "output_tokens": 600, "saved_tokens": 400},
                {"date": "2026-09-11", "commands": 1, "input_tokens": "50", "saved_tokens": 10},
                {"commands": 9},
            ]
        })
        .to_string();
        let days = parse_days(&raw);
        assert_eq!(
            days,
            [
                RtkDay { date: "2026-09-11".into(), commands: 1, input: 50, saved: 10 },
                RtkDay { date: "2026-09-12".into(), commands: 2, input: 1000, saved: 400 },
            ]
        );
        assert!(parse_days(r#"{"summary":{"total_commands":0},"daily":[]}"#).is_empty());
        assert!(parse_days("não é json").is_empty());
    }

    /// O dia que ainda corre fica de fora: só os dias antes dele contam.
    #[test]
    fn only_the_closed_days_count() {
        let day = |date: &str| RtkDay { date: date.into(), commands: 1, input: 1, saved: 1 };
        let days = vec![day("2026-09-15"), day("2026-09-16"), day("2026-09-17"), day("2026-09-18")];
        let kept: Vec<String> = closed(days, "2026-09-17").into_iter().map(|d| d.date).collect();
        assert_eq!(kept, ["2026-09-15", "2026-09-16"]);
    }

    #[test]
    fn num_reads_string_and_numeric() {
        let v = json!({ "a": "12.5", "b": 7, "c": "nope" });
        assert!((num(&v, &["a"]) - 12.5).abs() < f64::EPSILON);
        assert!((num(&v, &["b"]) - 7.0).abs() < f64::EPSILON);
        assert_eq!(num(&v, &["c"]), 0.0);
        assert_eq!(num(&v, &["missing"]), 0.0);
    }
}
