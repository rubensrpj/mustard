//! Source first, current scan second. Never execute shell text from a request.
//! A missing/stale/partial index cannot remove native occurrences.
use crate::domain::code_search::Request;
use crate::domain::knowledge::selection::SymbolSelector;
use crate::io::knowledge::investigation::{Occurrence, cross_hits_for};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Stdio;

pub mod presentation;
mod quality;

pub struct Answer {
    pub report: Value,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

pub fn execute(
    root: &Path,
    tree: &Path,
    cwd: &Path,
    request: &Request,
    selector: Option<&dyn SymbolSelector>,
) -> Result<Answer, String> {
    request.validate()?;
    // Native execution deliberately precedes every database access and Choice.
    let mut answer = if request.tool == "Read" {
        read(cwd, request)?
    } else {
        run(cwd, request)?
    };
    enrich(root, tree, cwd, request, &mut answer, selector);
    Ok(answer)
}

/// Reuse the executed result after refreshing the scan; never rerun the search.
pub fn enrich(
    root: &Path,
    tree: &Path,
    cwd: &Path,
    request: &Request,
    answer: &mut Answer,
    selector: Option<&dyn SymbolSelector>,
) {
    let hits = occurrences(tree, cwd, request, &answer.report["result"], &answer.stdout);
    if answer.report.get("learning").is_none() {
        answer.report["learning"] = crate::io::knowledge::observations::record(root, tree, &hits)
            .unwrap_or_else(
                |reason| json!({"status":"not-stored","reason":reason,"needs_scan":false}),
            );
    }
    let borrowed: Vec<_> = hits
        .iter()
        .map(|(file, line, text)| Occurrence {
            file,
            line: *line,
            text,
        })
        .collect();
    let crossed = (!borrowed.is_empty()).then(|| {
        cross_hits_for(
            root,
            tree,
            &borrowed,
            &request.intent,
            request.purpose,
            if request.choose { selector } else { None },
        )
    });
    let (evidence, status) = match crossed {
        Some(Ok(report))
            if ["symbols", "files"]
                .iter()
                .any(|key| report[key].as_array().is_some_and(|v| !v.is_empty())) =>
        {
            (report, "enriched")
        }
        Some(Ok(_)) => (Value::Null, "no-current-index-owner"),
        Some(Err(_)) => (Value::Null, "index-unavailable-or-stale"),
        None => (Value::Null, "no-crossable-occurrences"),
    };
    answer.report["schema_version"] = json!(1);
    answer.report["ok"] =
        json!(answer.exit_code == 0 || answer.exit_code == 1 && request.tool != "Read");
    answer.report["tool"] = json!(request.tool);
    answer.report["exit_code"] = json!(answer.exit_code);
    answer.report["intent"] = json!(request.intent);
    answer.report["purpose"] = json!(request.purpose);
    answer.report["evidence"] = evidence;
    answer.report["crossing_status"] = json!(status);
    answer.report["native_result_preserved"] = json!(true);
    answer.report["local_model_calls"] = json!(0);
    if quality::is_file_discovery(request) {
        answer.report["query_quality"] = quality::assess(root, answer);
    }
    answer.report["remote_model_calls"] = answer.report["evidence"]
        .get("remote_model_calls")
        .cloned()
        .unwrap_or_else(|| json!(0));
}

fn number(input: &Value, key: &str, default: u64) -> Result<u64, String> {
    input.get(key).map_or(Ok(default), |v| {
        v.as_u64().ok_or_else(|| format!("search-invalid-{key}"))
    })
}
fn text<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("search-missing-{key}"))
}
fn fields(input: &Value, allowed: &[&str]) -> Result<(), String> {
    let object = input.as_object().ok_or("search-input-object-required")?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("search-unsupported-option; use the original host tool".into());
    }
    Ok(())
}

fn search_path(input: &Value) -> Result<&str, String> {
    input.get("path").map_or(Ok("."), |value| {
        value
            .as_str()
            .ok_or_else(|| "search-path-string-required".into())
    })
}

fn read(cwd: &Path, request: &Request) -> Result<Answer, String> {
    fields(&request.input, &["file_path", "offset", "limit"])?;
    let file = text(&request.input, "file_path")?;
    let bytes = std::fs::read(cwd.join(file)).map_err(|e| e.to_string())?;
    let content =
        String::from_utf8(bytes).map_err(|_| "search-non-text-read; use the original host tool")?;
    if content.contains('\0') {
        return Err("search-non-text-read; use the original host tool".into());
    }
    let offset = number(&request.input, "offset", 1)?;
    if offset == 0 {
        return Err("search-offset-starts-at-one".into());
    }
    let limit = number(&request.input, "limit", 2000)?;
    let lines: Vec<_> = content
        .lines()
        .skip(offset.saturating_sub(1) as usize)
        .take(limit as usize)
        .collect();
    let result = json!({"file_path":file,"offset":offset,"numLines":lines.len(),"totalLines":content.lines().count(),
        "content":lines.join("\n")});
    Ok(Answer {
        stdout: lines.join("\n").into_bytes(),
        stderr: Vec::new(),
        exit_code: 0,
        report: json!({"result":result}),
    })
}

fn run(cwd: &Path, request: &Request) -> Result<Answer, String> {
    let (program, args) = arguments(request)?;
    let output = crate::platform::process::command(&program)
        .args(&args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let exit_code = output.status.code().unwrap_or(2);
    let stdout = output.stdout;
    let stderr = output.stderr;
    let result = if request.tool == "Grep" {
        grep_result(request, &stdout)?
    } else if request.tool == "Glob" {
        let files = String::from_utf8_lossy(&stdout)
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        json!({"filenames":files,"numFiles":files.len(),"truncated":false})
    } else {
        json!({"stdout":String::from_utf8_lossy(&stdout),"stderr":String::from_utf8_lossy(&stderr),"interrupted":false,"isImage":false})
    };
    Ok(Answer {
        report: json!({"result":result}),
        stdout,
        stderr,
        exit_code,
    })
}

pub fn arguments(request: &Request) -> Result<(String, Vec<String>), String> {
    request.validate()?;
    let input = &request.input;
    if matches!(request.tool.as_str(), "rg" | "grep" | "git") {
        fields(input, &["args"])?;
        let args: Vec<String> = input["args"]
            .as_array()
            .ok_or("search-args-array-required")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "search-string-argument-required".to_string())
            })
            .collect::<Result<_, _>>()?;
        // These options invoke another program. They are outside a read/search gateway.
        if args
            .iter()
            .any(|arg| matches!(arg.split('=').next(), Some("--pre" | "--hostname-bin")))
            || request.tool == "git"
                && (args.first().map(String::as_str) != Some("grep")
                    || args.iter().any(|arg| {
                        arg.starts_with("-O")
                            || arg.starts_with("--open-files-in-pager")
                            || arg == "--textconv"
                            || arg == "--ext-grep"
                    }))
        {
            return Err("search-executable-option-unsupported; use the original host tool".into());
        }
        return Ok((request.tool.clone(), args));
    }
    if request.tool == "Glob" {
        fields(input, &["pattern", "path"])?;
        return Ok((
            "rg".into(),
            vec![
                "--files".into(),
                "--hidden".into(),
                "-g".into(),
                "!.git/**".into(),
                "-g".into(),
                text(input, "pattern")?.into(),
                "--".into(),
                search_path(input)?.into(),
            ],
        ));
    }
    if request.tool != "Grep" {
        return Err("search-tool-has-no-native-arguments".into());
    }
    fields(
        input,
        &[
            "pattern",
            "path",
            "glob",
            "type",
            "output_mode",
            "-A",
            "-B",
            "-C",
            "context",
            "-n",
            "-i",
            "head_limit",
            "offset",
            "multiline",
        ],
    )?;
    number(input, "head_limit", 200)?;
    number(input, "offset", 0)?;
    let mode = input
        .get("output_mode")
        .map_or(Ok("files_with_matches"), |value| {
            value.as_str().ok_or("search-output-mode-string-required")
        })?;
    let mut args = vec![
        "--color=never".into(),
        "--no-heading".into(),
        "--with-filename".into(),
    ];
    match mode {
        "content" => {}
        "files_with_matches" => args.push("-l".into()),
        "count" => args.push("-c".into()),
        _ => return Err("search-output-mode-unsupported".into()),
    }
    for (field, flag) in [("-i", "-i"), ("-n", "-n"), ("multiline", "-U")] {
        if let Some(value) = input.get(field) {
            let value = value.as_bool().ok_or("search-boolean-required")?;
            if value {
                args.push(flag.into());
                if field == "multiline" {
                    args.push("--multiline-dotall".into());
                }
            }
        } else if field == "-n" {
            args.push(flag.into());
        }
    }
    for (field, flag) in [("glob", "--glob"), ("type", "--type")] {
        if let Some(value) = input.get(field) {
            args.extend([
                flag.into(),
                value.as_str().ok_or("search-string-required")?.into(),
            ]);
        }
    }
    for (field, flag) in [("-A", "-A"), ("-B", "-B"), ("-C", "-C"), ("context", "-C")] {
        if input.get(field).is_some() {
            args.extend([flag.into(), number(input, field, 0)?.to_string()]);
        }
    }
    args.extend([
        "-e".into(),
        text(input, "pattern")?.into(),
        "--".into(),
        search_path(input)?.into(),
    ]);
    Ok(("rg".into(), args))
}

fn grep_result(request: &Request, stdout: &[u8]) -> Result<Value, String> {
    let input = &request.input;
    let offset = number(input, "offset", 0)? as usize;
    let limit = number(input, "head_limit", 200)? as usize;
    let text = String::from_utf8_lossy(stdout);
    let all: Vec<_> = text.lines().collect();
    let selected: Vec<_> = all
        .iter()
        .skip(offset)
        .take(if limit == 0 { usize::MAX } else { limit })
        .copied()
        .collect();
    let truncated = offset + selected.len() < all.len();
    let mode = input["output_mode"]
        .as_str()
        .unwrap_or("files_with_matches");
    Ok(match mode {
        "files_with_matches" => {
            json!({"mode":mode,"filenames":selected,"numFiles":selected.len(),"truncated":truncated})
        }
        "count" => {
            json!({"mode":mode,"content":selected.join("\n"),"numLines":selected.len(),"truncated":truncated})
        }
        _ => {
            json!({"mode":mode,"content":selected.join("\n"),"numLines":selected.len(),"truncated":truncated})
        }
    })
}

fn occurrences(
    tree: &Path,
    cwd: &Path,
    request: &Request,
    result: &Value,
    stdout: &[u8],
) -> Vec<(String, u64, String)> {
    let mut hits = Vec::new();
    let mut push = |file: &str, line: u64, text: &str| {
        if hits.len() >= 256 {
            return;
        }
        let Ok(path) = cwd.join(file).canonicalize() else {
            return;
        };
        let Ok(relative) = path.strip_prefix(tree) else {
            return;
        };
        hits.push((
            relative.to_string_lossy().replace('\\', "/"),
            line,
            text.to_string(),
        ));
    };
    if request.tool == "Read" {
        let Some(file) = result["file_path"].as_str() else {
            return hits;
        };
        for (at, text) in result["content"]
            .as_str()
            .unwrap_or_default()
            .lines()
            .enumerate()
        {
            push(
                file,
                result["offset"].as_u64().unwrap_or(1) + at as u64,
                text,
            );
        }
        return hits;
    }
    let native_files = request.tool == "rg"
        && request.input["args"]
            .as_array()
            .is_some_and(|args| args.iter().any(|v| v == "--files"));
    if request.tool == "Glob"
        || native_files
        || request.tool == "Grep" && result["mode"] == "files_with_matches"
    {
        let text = String::from_utf8_lossy(stdout);
        let listed: Vec<_> = if native_files {
            text.lines().collect()
        } else {
            result["filenames"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect()
        };
        for file in listed.into_iter().take(96) {
            if std::fs::metadata(cwd.join(file)).is_ok_and(|meta| meta.len() <= 2 * 1024 * 1024)
                && let Ok(content) = std::fs::read_to_string(cwd.join(file))
                && let Some(text) = content.lines().next()
            {
                push(file, 1, text);
            }
        }
        return hits;
    }
    // Only content output with actual filename/line/text coordinates is crossable.
    // Counts, binary notices, custom separators and context rows stay native.
    let text = if request.tool == "Grep" {
        result["content"].as_str().unwrap_or_default().to_string()
    } else {
        String::from_utf8_lossy(stdout).into_owned()
    };
    for row in text.lines() {
        for (at, _) in row.match_indices(':') {
            let file = &row[..at];
            let Some((line, text)) = row[at + 1..].split_once(':') else {
                continue;
            };
            if let Ok(line) = line.parse::<u64>()
                && cwd.join(file).is_file()
            {
                push(file, line, text);
                break;
            }
        }
    }
    hits
}
