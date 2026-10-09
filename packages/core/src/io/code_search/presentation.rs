//! Agent output is an alternative representation, never native output plus a
//! diagnostic report. Compact only when every record can be restored in order.
use super::Answer;
use crate::domain::code_search::Request;
use std::collections::BTreeSet;
use std::path::Path;

pub struct Presentation {
    pub stdout: Vec<u8>,
    pub representation: &'static str,
    pub owner_ranges: usize,
}

pub fn agent(answer: &Answer, request: &Request, cwd: &Path) -> Presentation {
    let native = matches!(request.tool.as_str(), "rg" | "grep" | "git");
    let original = if native {
        answer.stdout.clone()
    } else {
        // Preserve the actual paginated adapter result, not the subprocess's
        // unpaginated stdout. Read retains its path/offset/totalLines contract.
        let mut bytes = answer.report["result"].to_string().into_bytes();
        bytes.push(b'\n');
        bytes
    };
    let fallback = || Presentation {
        stdout: original.clone(),
        representation: if native { "native" } else { "tool-result" },
        owner_ranges: 0,
    };
    if answer.exit_code != 0 || !(native || request.tool == "Grep") {
        return fallback();
    }
    // A filename or count can itself contain ":12:". Those modes are not
    // coordinate records, even when their text happens to fit the grammar.
    if native
        && request.input["args"].as_array().is_some_and(|args| {
            args.iter()
                .filter_map(serde_json::Value::as_str)
                .any(|arg| {
                    matches!(
                        arg.split('=').next().unwrap_or_default(),
                        "--files"
                            | "--files-with-matches"
                            | "--files-without-match"
                            | "--files-without-matches"
                            | "--count"
                            | "--count-matches"
                            | "--json"
                            | "--heading"
                            | "--no-filename"
                            | "--null"
                            | "--null-data"
                            | "--vimgrep"
                            | "-c"
                            | "-l"
                            | "-L"
                            | "-h"
                            | "-0"
                            | "-z"
                    ) || arg.strip_prefix('-').is_some_and(|flags| {
                        !flags.starts_with('-') && flags.contains(['c', 'l', 'L', 'h', '0', 'z'])
                    })
                })
        })
    {
        return fallback();
    }
    if request.tool == "Grep" && answer.report["result"]["mode"] != "content" {
        return fallback();
    }
    let text = if native {
        let Ok(text) = std::str::from_utf8(&answer.stdout) else {
            return fallback();
        };
        text
    } else {
        answer.report["result"]["content"]
            .as_str()
            .unwrap_or_default()
    };
    if text.is_empty() || text.contains(['\r', '\0', '\x1b']) {
        return fallback();
    }
    let Some(records) = records(text) else {
        return fallback();
    };
    let page = if !native {
        format!(
            "# page offset {}; more {}\n",
            request.input["offset"].as_u64().unwrap_or(0),
            answer.report["result"]["truncated"]
                .as_bool()
                .unwrap_or(false)
        )
    } else {
        String::new()
    };
    let (enriched, count) = grouped(answer, cwd, &records, &page, text.ends_with('\n'), true);
    // An explicit Choice request asks for a recommendation in addition to
    // matches. Ordinary searches never grow just to carry scan metadata.
    let explicit_selection = request.choose
        && answer.report["evidence"]["recommended_symbols"]
            .as_array()
            .is_some_and(|ids| !ids.is_empty());
    if count > 0 && (enriched.len() <= original.len() || explicit_selection) {
        return Presentation {
            stdout: enriched.into_bytes(),
            representation: "grouped-current-owners",
            owner_ranges: count,
        };
    }
    let (compact, _) = grouped(answer, cwd, &records, &page, text.ends_with('\n'), false);
    if compact.len() < original.len() {
        return Presentation {
            stdout: compact.into_bytes(),
            representation: "grouped-matches",
            owner_ranges: 0,
        };
    }
    fallback()
}

struct Record<'a> {
    file: &'a str,
    line: u64,
    // Keep original digits/text intact, including duplicate matches and spaces.
    rest: &'a str,
}

fn records(text: &str) -> Option<Vec<Record<'_>>> {
    text.lines()
        .map(|row| {
            row.match_indices(':').find_map(|(at, _)| {
                let file = &row[..at];
                let rest = &row[at + 1..];
                let (number, _) = rest.split_once(':')?;
                if file.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                let line = number.parse::<u64>().ok().filter(|line| *line > 0)?;
                Some(Record { file, line, rest })
            })
        })
        .collect()
}

fn grouped(
    answer: &Answer,
    cwd: &Path,
    records: &[Record<'_>],
    page: &str,
    trailing_newline: bool,
    owners: bool,
) -> (String, usize) {
    let mut result = page.to_string();
    let mut previous = None;
    let mut seen = BTreeSet::new();
    let mut count = 0;
    for record in records {
        if previous != Some(record.file) {
            result.push_str("@ ");
            result.push_str(record.file);
            result.push('\n');
            if owners && seen.insert(record.file) {
                let ranges: Vec<_> = answer.report["evidence"]["symbols"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|card| {
                        card["source"]["file"] == record.file
                            || card["read"]["input"]["file_path"]
                                .as_str()
                                .is_some_and(|file| cwd.join(record.file) == Path::new(file))
                    })
                    .filter(|card| {
                        card["matched_lines"].as_array().is_some_and(|lines| {
                            records.iter().any(|row| {
                                row.file == record.file && lines.iter().any(|n| n == row.line)
                            })
                        })
                    })
                    .filter_map(|card| {
                        let name = card["name"].as_str()?;
                        let start = card["source"]["line"].as_u64()?;
                        let end = card["source"]["end_line"].as_u64()?;
                        if name.contains(['\n', '\r']) || start == 0 || end < start {
                            return None;
                        }
                        let recommended = answer.report["evidence"]["recommended_symbols"]
                            .as_array()
                            .is_some_and(|ids| ids.contains(&card["id"]));
                        Some(format!(
                            "{name} {start}-{end}{}",
                            if recommended { " [recommended]" } else { "" }
                        ))
                    })
                    .collect();
                if !ranges.is_empty() {
                    result.push_str("# static owners: ");
                    result.push_str(&ranges.join("; "));
                    result.push('\n');
                    count += ranges.len();
                }
            }
            previous = Some(record.file);
        }
        result.push_str(record.rest);
        result.push('\n');
    }
    if !trailing_newline {
        result.pop();
    }
    (result, count)
}
