//! Explicit investigations through the host-independent gateway. A source path
//! is mandatory, so an identity cannot silently change the requested scope.
use super::{Answer, fields, number, text};
use crate::domain::code_search::Request;
use crate::domain::knowledge::resources::Registry;
use crate::io::knowledge::{self, Direction, Query};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

pub fn requested(request: &Request) -> bool {
    matches!(
        request.tool.as_str(),
        "Symbol" | "Trace" | "Structure" | "References"
    )
}

pub fn execute(root: &Path, tree: &Path, cwd: &Path, request: &Request) -> Result<Answer, String> {
    request.validate()?;
    let supplied = text(&request.input, "file_path")?;
    let canonical = cwd
        .join(supplied)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let tree = tree.canonicalize().map_err(|e| e.to_string())?;
    let relative = canonical
        .strip_prefix(&tree)
        .map_err(|_| "search-source-outside-tree")?;
    let file = relative
        .to_str()
        .ok_or("search-source-path-encoding")?
        .replace('\\', "/");
    let registry = Registry::load()?;
    let source = knowledge::investigation::safe_read(&tree, &file, &registry)
        .ok_or("search-source-excluded-or-unreadable")?;
    let mut result = match request.tool.as_str() {
        "Structure" => {
            fields(&request.input, &["file_path", "query"])?;
            let result = crate::Scan::locate()
                .structure(&tree, &file, text(&request.input, "query")?)
                .map_err(|e| e.to_string())?;
            // A source change while the external parser runs invalidates its receipts.
            if knowledge::investigation::safe_read(&tree, &file, &registry).as_deref()
                != Some(&source)
            {
                return Err("search-source-changed-during-parse".into());
            }
            result
        }
        "References" => {
            fields(
                &request.input,
                &["file_path", "line", "column", "relation", "limit"],
            )?;
            for key in ["line", "column"] {
                if request.input.get(key).is_none() {
                    return Err(format!("search-missing-{key}"));
                }
            }
            let line = number(&request.input, "line", 1)?;
            let column = number(&request.input, "column", 0)?;
            let row = source
                .split('\n')
                .nth(
                    usize::try_from(line.checked_sub(1).ok_or("search-line-starts-at-one")?)
                        .map_err(|_| "search-invalid-line")?,
                )
                .ok_or("search-line-outside-source")?;
            if !row.is_char_boundary(usize::try_from(column).map_err(|_| "search-invalid-column")?)
            {
                return Err("search-invalid-byte-column".into());
            }
            let mut result = knowledge::precise::references(
                root,
                &tree,
                &file,
                number(&request.input, "line", 1)?,
                number(&request.input, "column", 0)?,
                request
                    .input
                    .get("relation")
                    .map(|_| text(&request.input, "relation"))
                    .transpose()?
                    .unwrap_or("references"),
                number(&request.input, "limit", 64)? as usize,
            )?;
            if result["references"].as_array().is_none_or(Vec::is_empty) {
                // Text candidates do not prove references or absence of uses.
                // Preserve the actual native page when a precise provider is
                // unavailable or has no in-project locations.
                let line = number(&request.input, "line", 1)?;
                let column = number(&request.input, "column", 0)?;
                if let Some(token) = identifier_at(&source, line, column) {
                    let fallback = Request {
                        tool: "Grep".into(),
                        input: json!({"pattern":regex_literal(&token),"path":".","output_mode":"content","head_limit":number(&request.input,"limit",64)?.clamp(1,256),"-n":true}),
                        intent: request.intent.clone(),
                        purpose: crate::domain::knowledge::investigation::Purpose::Locate,
                        choose: false,
                    };
                    let native = super::execute_native(&tree, &fallback)?;
                    result["native_fallback"] = json!({"request":fallback,"result":native.report["result"],
                        "stdout":String::from_utf8_lossy(&native.stdout),"stderr":String::from_utf8_lossy(&native.stderr),"exit_code":native.exit_code,
                        "meaning":"literal candidates; not verified references"});
                }
            }
            result
        }
        "Symbol" | "Trace" => {
            fields(
                &request.input,
                &[
                    "file_path",
                    "symbol",
                    "direction",
                    "depth",
                    "limit",
                    "target",
                ],
            )?;
            let symbol = text(&request.input, "symbol")?;
            if !symbol.starts_with(&format!("{file}:")) {
                return Err("search-symbol-outside-explicit-file".into());
            }
            let direction = match request
                .input
                .get("direction")
                .map(|_| text(&request.input, "direction"))
                .transpose()?
                .unwrap_or("outgoing")
            {
                "outgoing" => Direction::Outgoing,
                "callers" => Direction::Callers,
                "both" => Direction::Both,
                _ => return Err("search-invalid-direction".into()),
            };
            let query = Query {
                text: "",
                file: None,
                limit: number(&request.input, "limit", 24)?.clamp(1, 128) as usize,
                depth: if request.tool == "Symbol" {
                    0
                } else {
                    number(&request.input, "depth", 3)?.min(4) as usize
                },
                all: false,
                detail: true,
                symbol: Some(symbol),
                direction,
                refresh: false,
            };
            let (report, _) =
                knowledge::query_with(root, &tree, &query).map_err(|e| format!("{e:?}"))?;
            let mut cards = report["cards"].as_array().cloned().unwrap_or_default();
            if !cards.iter().any(|card| card["id"] == symbol) {
                return Err("search-symbol-not-current; locate again".into());
            }
            let mut paths = report["navigation"]["paths"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let target = request
                .input
                .get("target")
                .map(|_| text(&request.input, "target"))
                .transpose()?;
            let mut target_reached = Value::Null;
            if let Some(target) = target {
                let mut keep = BTreeSet::from([target.to_string()]);
                for path in paths.iter().rev() {
                    if path["to"].as_str().is_some_and(|id| keep.contains(id)) {
                        keep.insert(path["from"].as_str().unwrap_or_default().to_string());
                    }
                }
                let reached = keep.contains(symbol);
                target_reached = json!(reached);
                paths.retain(|path| {
                    reached && path["to"].as_str().is_some_and(|id| keep.contains(id))
                });
                cards.retain(|card| {
                    card["id"] == symbol
                        || reached && card["id"].as_str().is_some_and(|id| keep.contains(id))
                });
            }
            // Return compact coordinates/signatures, leaving bodies to the existing
            // named investigation or a scoped Read, instead of duplicating them.
            let cards:Vec<_>=cards.into_iter().filter(|card|card["source"]["file"].as_str()
                .is_some_and(|file|knowledge::investigation::safe_read(&tree,file,&registry).is_some()))
                .map(|card|json!({"id":card["id"],"name":card["name"],"kind":card["kind"],
                    "source":card["source"],"signature":card["signature"],"syntax":card["syntax"],"parse_complete":card["parse_complete"]})).collect();
            let allowed: BTreeSet<_> = cards
                .iter()
                .filter_map(|card| card["id"].as_str())
                .collect();
            paths.retain(|path| {
                path["from"].as_str().is_some_and(|id| allowed.contains(id))
                    && path["to"].as_str().is_some_and(|id| allowed.contains(id))
            });
            let mut navigation = report["navigation"].clone();
            navigation.as_object_mut().map(|n| n.remove("paths"));
            json!({"symbol":symbol,"cards":cards,"paths":paths,"target":target,"target_reached":target_reached,
                "navigation":navigation,"gaps":report["gaps"],
                "meaning":"static connections; not execution order or proven data flow",
                "interpretations":report["interpretations"]})
        }
        _ => return Err("search-tool-unsupported".into()),
    };
    let hits = current_hits(&tree, &result)?;
    let learning = knowledge::observations::record(root, &tree, &hits)
        .unwrap_or_else(|reason| json!({"status":"not-stored","reason":reason}));
    result["owners"] = owners(root, &tree, &hits);
    let stdout = result.to_string().into_bytes();
    Ok(Answer {
        report: json!({"schema_version":1,"ok":true,"tool":request.tool,"result":result,
        "learning":learning,"local_model_calls":0,"remote_model_calls":0}),
        stdout,
        stderr: vec![],
        exit_code: 0,
    })
}

/// Reuse the executed result after an incremental scan; never repeat a parser
/// or compiler request simply to attach newly indexed owners.
pub fn recross(root: &Path, tree: &Path, answer: &mut Answer) -> Result<(), String> {
    let hits = current_hits(tree, &answer.report["result"])?;
    answer.report["result"]["owners"] = owners(root, tree, &hits);
    answer.stdout = answer.report["result"].to_string().into_bytes();
    Ok(())
}

fn current_hits(tree: &Path, result: &Value) -> Result<Vec<(String, u64, String)>, String> {
    let registry = Registry::load()?;
    let mut positions = BTreeSet::new();
    for capture in result["matches"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|m| m["captures"].as_array().into_iter().flatten())
    {
        if let (Some(file), Some(sha), Some(line)) = (
            result["file"].as_str(),
            result["sha256"].as_str(),
            capture["line"].as_u64(),
        ) {
            positions.insert((file.to_string(), line, sha.to_string()));
        }
    }
    for entry in result["cards"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(result["references"].as_array().into_iter().flatten())
    {
        if let (Some(file), Some(line), Some(sha)) = (
            entry["source"]["file"].as_str(),
            entry["source"]["line"].as_u64(),
            entry["source"]["sha256"].as_str(),
        ) {
            positions.insert((file.to_string(), line, sha.to_string()));
        }
    }
    let mut hits = Vec::new();
    for (file, line, expected) in positions {
        let text = knowledge::investigation::safe_read(tree, &file, &registry)
            .ok_or("search-source-excluded-or-unreadable")?;
        let mut hash = crate::io::sha256::Sha256::new();
        hash.update(text.as_bytes());
        if hash.hex_digest() != expected {
            return Err("search-source-changed-before-crossing".into());
        }
        let row = text
            .lines()
            .nth(line.checked_sub(1).ok_or("search-invalid-line")? as usize)
            .ok_or("search-line-outside-source")?;
        hits.push((file, line, row.to_string()));
    }
    Ok(hits)
}

fn owners(root: &Path, tree: &Path, hits: &[(String, u64, String)]) -> Value {
    let borrowed: Vec<_> = hits
        .iter()
        .map(|(file, line, text)| knowledge::investigation::Occurrence {
            file,
            line: *line,
            text,
        })
        .collect();
    knowledge::investigation::cross_hits(root, tree, &borrowed)
        .unwrap_or_else(|_| json!({"status":"no-current-index-owners","symbols":[]}))
}

fn identifier_at(source: &str, line: u64, column: u64) -> Option<String> {
    let text = source
        .lines()
        .nth(usize::try_from(line.checked_sub(1)?).ok()?)?;
    let column = usize::try_from(column).ok()?;
    let left = text.get(..column)?;
    let right = text.get(column..)?;
    let identifier = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let start = left
        .char_indices()
        .rev()
        .take_while(|(_, c)| identifier(*c))
        .last()
        .map_or(column, |(at, _)| at);
    let end = column
        + right
            .chars()
            .take_while(|c| identifier(*c))
            .map(char::len_utf8)
            .sum::<usize>();
    (start < end).then(|| text[start..end].to_string())
}
fn regex_literal(text: &str) -> String {
    let mut escaped = String::new();
    for c in text.chars() {
        if ".*+?()[]{}^$|\\".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}
