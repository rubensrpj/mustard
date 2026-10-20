//! Source first, current scan second. Never execute shell text from a request.
//! A missing/stale/partial index cannot remove native occurrences.
use crate::domain::code_search::Request;
use crate::domain::knowledge::selection::SymbolSelector;
use crate::io::knowledge::investigation::{Occurrence, cross_hits_for};
use crate::io::knowledge::observations::{MAX_SOURCE_BYTES, MAX_SOURCES};
use crate::io::map_db::{Kept, Later};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Stdio;

pub mod operations;
pub mod presentation;
pub mod delivery;
mod chain;
mod quality;
mod scope;
mod task;
mod discovery;
mod task_view;
mod source_view;

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
    let mut later = Later::default();
    let mut answer = if operations::requested(request) {
        operations::execute(root, tree, cwd, request, &mut later)?
    } else {
        let mut answer = execute_native(cwd, request)?;
        enrich(root, tree, cwd, request, &mut answer, selector, &mut later);
        answer
    };
    // Quem chama a biblioteca sem prazo grava o que aprendeu esperando a
    // trava como toda gravação.
    settle(&mut answer, &later.keep(crate::io::map_db::BUSY_WAIT));
    Ok(answer)
}

/// Diz no relato como acabou a gravação do que a busca aprendeu. A que ficou
/// para a próxima busca tira dos relatos de aprendizado o "gravado".
pub fn settle(answer: &mut Answer, kept: &Kept) {
    answer.report["learning_write"] = kept.report();
    if kept.left() {
        for pointer in ["/learning", "/task_context/learning"] {
            if let Some(learning) = answer.report.pointer_mut(pointer).filter(|learning| learning["status"] == "stored-current-source-facts") {
                learning["status"] = json!("not-stored");
                learning["reason"] = json!("map-busy; the next search learns it again");
            }
        }
    }
}

/// The runtime can refresh the scan between native execution/observation and
/// investigation without running the original search or judgement twice.
pub fn execute_native(cwd: &Path, request: &Request) -> Result<Answer, String> {
    request.validate()?;
    if request.tool == "Read" {read(cwd,request)} else {run(cwd,request)}
}

/// O que o resultado nativo ensina, junto em `later`. A saída longa se lê só
/// até o teto de fontes do aprendizado: o resto não entraria nele.
pub fn observe(root: &Path, tree: &Path, cwd: &Path, request: &Request, answer: &mut Answer, later: &mut Later) {
    if answer.report.get("learning").is_none() {
        let (hits,cut)=read_occurrences(tree,cwd,request,&answer.report["result"],&answer.stdout,MAX_SOURCES,MAX_SOURCE_BYTES);
        let mut learning=crate::io::knowledge::observations::learn(root,tree,&hits,later)
            .unwrap_or_else(|reason|json!({"status":"not-stored","reason":reason,"needs_scan":false}));
        learning["occurrences"]=json!({"read":hits.len(),"cut":cut});
        answer.report["learning"]=learning;
    }
}

/// Reuse the executed result after refreshing the scan; never rerun the search.
/// O mapa com bloco a pôr em dia ([`crate::io::project_map::needs_block_update`])
/// não se cruza nem se investiga: abri-lo refaria o bloco, que é da passada do
/// scan, e com ela rodando cada abertura esperaria a trava dela. A resposta
/// fica com o resultado nativo e diz que o índice não está em dia.
pub fn enrich(
    root: &Path,
    tree: &Path,
    cwd: &Path,
    request: &Request,
    answer: &mut Answer,
    selector: Option<&dyn SymbolSelector>,
    later: &mut Later,
) {
    let hits = occurrences(tree, cwd, request, &answer.report["result"], &answer.stdout);
    observe(root,tree,cwd,request,answer,later);
    let task_requested=task::requested(request) && matches!(answer.exit_code,0|1);
    let outdated = crate::io::project_map::needs_block_update(&crate::io::project_map::model_path(root));
    let borrowed: Vec<_> = hits
        .iter()
        .map(|(file, line, text)| Occurrence {
            file,
            line: *line,
            text,
        })
        .collect();
    // `Some(None)`: havia o que cruzar, e o mapa não deu como.
    let crossed = (!borrowed.is_empty()).then(|| {
        (!outdated)
            .then(|| {
                cross_hits_for(
                    root,
                    tree,
                    &borrowed,
                    &request.intent,
                    request.purpose,
                    if request.choose && !task_requested { selector } else { None },
                )
            })
            .and_then(Result::ok)
    });
    let (evidence, status) = match crossed {
        Some(Some(report))
            if ["symbols", "files"]
                .iter()
                .any(|key| report[key].as_array().is_some_and(|v| !v.is_empty())) =>
        {
            (report, "enriched")
        }
        Some(Some(_)) => (Value::Null, "no-current-index-owner"),
        Some(None) => (Value::Null, "index-unavailable-or-stale"),
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
    if task_requested {
        // A failed literal search is not evidence that unrelated lexical or
        // vector neighbours answer it. Preserve the empty native result; the
        // caller can explicitly refine its pattern or query knowledge.
        if answer.exit_code == 1 && answer.stdout.is_empty() {
            answer.report["task_context"]=json!({"status":"native-fallback","reason":"native-no-matches; refine the original pattern or explicitly query knowledge","remote_model_calls":0});
            return;
        }
        if outdated {
            answer.report["task_context"]=json!({"status":"native-fallback","reason":"map-block-outdated; the scan pass rebuilds it","remote_model_calls":0});
            return;
        }
        match task::investigate(root,tree,cwd,request,answer,if request.choose {selector}else{None},later) {
            Ok(context)=>{
                answer.report["remote_model_calls"]=context["remote_model_calls"].clone();
                answer.report["local_model_calls"]=context["local_model_calls"].clone();
                answer.report["task_context"]=context;
            },
            Err(reason)=>answer.report["task_context"]=json!({"status":"native-fallback","reason":reason,"remote_model_calls":0}),
        }
    }
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

/// The lines an executed search found, as the path from `tree`, the line and
/// its text. Only coordinate output (`file:line:text`) and listed files count.
pub fn occurrences(
    tree: &Path,
    cwd: &Path,
    request: &Request,
    result: &Value,
    stdout: &[u8],
) -> Vec<(String, u64, String)> {
    read_occurrences(tree, cwd, request, result, stdout, usize::MAX, usize::MAX).0
}

/// [`occurrences`] lidas só até a `files`-ésima fonte e os primeiros `bytes`
/// da saída: a linha de outra fonte depois disso, ou além desses bytes, não se
/// lê. Diz se cortou.
fn read_occurrences(
    tree: &Path,
    cwd: &Path,
    request: &Request,
    result: &Value,
    stdout: &[u8],
    files: usize,
    bytes: usize,
) -> (Vec<(String, u64, String)>, bool) {
    let mut hits = Vec::new();
    // Both sides of containment must use the same filesystem spelling (for
    // example macOS /var versus /private/var). A failed resolution admits no
    // indexed evidence while the already executed native result stays intact.
    let Ok(tree) = tree.canonicalize() else { return (hits, false); };
    let mut paths = std::collections::BTreeMap::<String, Option<String>>::new();
    let mut sources = std::collections::BTreeSet::<String>::new();
    // `false` quando a linha é de uma fonte além do teto.
    let mut push = |file: &str, line: u64, text: &str| {
        let relative=paths.entry(file.into()).or_insert_with(||cwd.join(file).canonicalize().ok()
            .and_then(|path|path.strip_prefix(&tree).ok().map(|path|path.to_string_lossy().replace('\\',"/"))));
        let Some(relative)=relative else {return true;};
        if !sources.contains(relative.as_str()) {
            if sources.len() >= files {return false;}
            sources.insert(relative.clone());
        }
        hits.push((
            relative.clone(),
            line,
            text.to_string(),
        ));
        true
    };
    if request.tool == "Read" {
        let Some(file) = result["file_path"].as_str() else {
            return (hits, false);
        };
        for (at, text) in result["content"]
            .as_str()
            .unwrap_or_default()
            .lines()
            .enumerate()
        {
            if !push(
                file,
                result["offset"].as_u64().unwrap_or(1) + at as u64,
                text,
            ) {
                return (hits, true);
            }
        }
        return (hits, false);
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
        let cut = listed.len() > files.min(96);
        for file in listed.into_iter().take(files.min(96)) {
            if std::fs::metadata(cwd.join(file)).is_ok_and(|meta| meta.len() <= 2 * 1024 * 1024)
                && let Ok(content) = std::fs::read_to_string(cwd.join(file))
                && let Some(text) = content.lines().next()
            {
                push(file, 1, text);
            }
        }
        return (hits, cut);
    }
    // Only content output with actual filename/line/text coordinates is crossable.
    // Counts, binary notices, custom separators and context rows stay native.
    let whole = if request.tool == "Grep" {
        result["content"].as_str().unwrap_or_default().as_bytes()
    } else {
        stdout
    };
    let head = whole_lines(whole, bytes);
    let mut cut = head.len() < whole.len();
    'rows: for row in String::from_utf8_lossy(head).lines() {
        for (at, _) in row.match_indices(':') {
            let file = &row[..at];
            let Some((line, text)) = row[at + 1..].split_once(':') else {
                continue;
            };
            if let Ok(line) = line.parse::<u64>()
                && cwd.join(file).is_file()
            {
                if !push(file, line, text) {
                    cut = true;
                    break 'rows;
                }
                break;
            }
        }
    }
    (hits, cut)
}

/// O começo de `output` até `bytes`, em linhas inteiras.
fn whole_lines(output: &[u8], bytes: usize) -> &[u8] {
    if output.len() <= bytes {
        return output;
    }
    output[..bytes].iter().rposition(|byte| *byte == b'\n').map_or(&[], |end| &output[..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A saída do rg com mais arquivos do que o aprendizado lê não é lida
    /// inteira para ele: a leitura para no teto de fontes, e o aprendizado
    /// diz que cortou. A resposta do rg fica inteira.
    #[test]
    fn a_native_output_past_the_learning_ceiling_is_read_only_up_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for at in 0..150 {
            std::fs::write(root.join(format!("m{at:03}.rs")), format!("pub fn quartz_{at}() {{}}\n")).unwrap();
        }
        let request = Request::native(&["rg".into(), "-n".into(), "--sort".into(), "path".into(), "quartz_".into(), ".".into()]).unwrap();
        let mut answer = execute_native(&root, &request).unwrap();
        assert_eq!(String::from_utf8_lossy(&answer.stdout).lines().count(), 150, "the native result stays whole");

        let mut later = Later::default();
        observe(&root, &root, &root, &request, &mut answer, &mut later);

        let learning = &answer.report["learning"];
        assert_eq!(learning["occurrences"]["cut"], true, "{}", learning["occurrences"]);
        assert_eq!(learning["occurrences"]["read"], 96, "{}", learning["occurrences"]);
        assert_eq!(learning["source_hashes"].as_object().map(serde_json::Map::len), Some(96));
        assert_eq!(String::from_utf8_lossy(&answer.stdout).lines().count(), 150, "the native result stays whole");
    }
}
